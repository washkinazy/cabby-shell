//! Bounded, event-driven PulseAudio-compatible device controls. No UI dependency.
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, ChildStdout},
};

const MAX_BYTES: usize = 1024 * 1024;
const MAX_EVENT: usize = 4096;
const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Timeout,
    InvalidData,
    Rejected,
    InvalidCommand,
    Disconnected,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "Audio service or pactl unavailable",
            Self::Timeout => "Audio request timed out",
            Self::InvalidData => "Audio service returned unsupported data",
            Self::Rejected => "Audio request was rejected",
            Self::InvalidCommand => "Audio device or value is no longer valid",
            Self::Disconnected => "Audio service disconnected",
        })
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Output,
    Input,
}
impl Kind {
    fn target(self) -> &'static str {
        match self {
            Self::Output => "sink",
            Self::Input => "source",
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct Device {
    pub kind: Kind,
    pub name: String,
    pub description: String,
    pub percent: u32,
    pub muted: bool,
}
impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub devices: Vec<Device>,
    pub default_output: Option<String>,
    pub default_input: Option<String>,
}
impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("devices", &self.devices.len())
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Default,
    Volume(u8),
    Mute(bool),
}
#[derive(Clone)]
pub struct Command {
    pub kind: Kind,
    pub name: String,
    pub operation: Operation,
}
impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Command")
            .field("kind", &self.kind)
            .field("operation", &self.operation)
            .finish_non_exhaustive()
    }
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 512
        && !name.starts_with(['-', '@'])
        && !name.chars().any(char::is_control)
}
impl Snapshot {
    pub fn is_default(&self, device: &Device) -> bool {
        let name = match device.kind {
            Kind::Output => &self.default_output,
            Kind::Input => &self.default_input,
        };
        name.as_deref() == Some(device.name.as_str())
    }
    pub fn arguments(&self, command: &Command) -> Result<Vec<String>> {
        if !valid_name(&command.name)
            || !self
                .devices
                .iter()
                .any(|d| d.kind == command.kind && d.name == command.name)
        {
            return Err(Error::InvalidCommand);
        }
        let target = command.kind.target();
        let (verb, value) = match command.operation {
            Operation::Default => (format!("set-default-{target}"), None),
            Operation::Volume(percent) if percent <= 100 => {
                (format!("set-{target}-volume"), Some(format!("{percent}%")))
            }
            Operation::Volume(_) => return Err(Error::InvalidCommand),
            Operation::Mute(muted) => (
                format!("set-{target}-mute"),
                Some(u8::from(muted).to_string()),
            ),
        };
        let mut args = vec![verb, command.name.clone()];
        args.extend(value);
        Ok(args)
    }
}
#[derive(Deserialize)]
struct RawDevice {
    name: String,
    description: Option<String>,
    mute: bool,
    volume: BTreeMap<String, RawVolume>,
    monitor_source: Option<String>,
}
#[derive(Deserialize)]
struct RawVolume {
    value: u32,
}
#[derive(Deserialize)]
struct Info {
    default_sink_name: Option<String>,
    default_source_name: Option<String>,
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::InvalidData);
    }
    serde_json::from_slice(bytes).map_err(|_| Error::InvalidData)
}
pub fn parse(info: &[u8], sinks: &[u8], sources: &[u8]) -> Result<Snapshot> {
    let info: Info = decode(info)?;
    let mut devices = Vec::new();
    for (kind, bytes) in [(Kind::Output, sinks), (Kind::Input, sources)] {
        let raw: Vec<RawDevice> = decode(bytes)?;
        if raw.len() > 256 {
            return Err(Error::InvalidData);
        }
        let mut names = BTreeSet::new();
        for item in raw {
            // pactl 17 uses monitor_source for a source's monitor-of-sink name too.
            if kind == Kind::Input && item.monitor_source.is_some() {
                continue;
            }
            if !valid_name(&item.name)
                || !names.insert(item.name.clone())
                || item.volume.is_empty()
                || item.volume.len() > 32
            {
                return Err(Error::InvalidData);
            }
            let percent = item
                .volume
                .values()
                .map(|v| (u64::from(v.value) * 100 + 32768) / 65536)
                .max()
                .unwrap() as u32;
            let description = item.description.unwrap_or_else(|| item.name.clone());
            if description.len() > 4096 {
                return Err(Error::InvalidData);
            }
            devices.push(Device {
                kind,
                name: item.name,
                description: description
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(160)
                    .collect(),
                percent,
                muted: item.mute,
            });
        }
    }
    let present = |kind, name: Option<String>| {
        name.filter(|n| devices.iter().any(|d| d.kind == kind && d.name == *n))
    };
    let default_output = present(Kind::Output, info.default_sink_name);
    let default_input = present(Kind::Input, info.default_source_name);
    Ok(Snapshot {
        devices,
        default_output,
        default_input,
    })
}

#[derive(Clone, Debug)]
pub struct Client {
    executable: PathBuf,
}
impl Default for Client {
    fn default() -> Self {
        Self {
            executable: "pactl".into(),
        }
    }
}
impl Client {
    /// Explicit dependency injection for isolated helpers/tests; not a user setting.
    pub fn with_executable(executable: PathBuf) -> Self {
        Self { executable }
    }
    fn process(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.executable);
        cmd.env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        cmd
    }
    async fn request(&self, args: &[String]) -> Result<Vec<u8>> {
        let mut child = self
            .process()
            .args(args)
            .spawn()
            .map_err(|_| Error::Unavailable)?;
        let result = tokio::time::timeout(TIMEOUT, async {
            let mut bytes = Vec::new();
            child
                .stdout
                .take()
                .ok_or(Error::Unavailable)?
                .take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| Error::Disconnected)?;
            if bytes.len() > MAX_BYTES {
                return Err(Error::InvalidData);
            }
            if !child
                .wait()
                .await
                .map_err(|_| Error::Disconnected)?
                .success()
            {
                return Err(Error::Rejected);
            }
            Ok(bytes)
        })
        .await
        .unwrap_or(Err(Error::Timeout));
        if result.is_err() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        result
    }
    async fn json(&self, args: &[&str]) -> Result<Vec<u8>> {
        let mut command = vec!["--format=json".to_owned()];
        command.extend(args.iter().map(|s| (*s).to_owned()));
        self.request(&command).await
    }
    pub async fn snapshot(&self) -> Result<Snapshot> {
        let info = self.json(&["info"]).await?;
        let sinks = self.json(&["list", "sinks"]).await?;
        let sources = self.json(&["list", "sources"]).await?;
        parse(&info, &sinks, &sources)
    }
    pub async fn apply(&self, snapshot: &Snapshot, command: &Command) -> Result<()> {
        self.request(&snapshot.arguments(command)?)
            .await
            .map(|_| ())
    }
    pub fn subscribe(&self) -> Result<Events> {
        let mut child = self
            .process()
            .args(["--format=json", "subscribe"])
            .spawn()
            .map_err(|_| Error::Unavailable)?;
        let stdout = child.stdout.take().ok_or(Error::Unavailable)?;
        Ok(Events {
            child,
            reader: BufReader::new(stdout),
            line: Vec::new(),
        })
    }
}
pub struct Events {
    child: Child,
    reader: BufReader<ChildStdout>,
    line: Vec<u8>,
}
impl Events {
    /// Ignore client/stream events to avoid snapshot helpers triggering themselves.
    pub async fn next(&mut self) -> Result<()> {
        loop {
            (&mut self.reader)
                .take((MAX_EVENT + 1 - self.line.len()) as u64)
                .read_until(b'\n', &mut self.line)
                .await
                .map_err(|_| Error::Disconnected)?;
            if self.line.is_empty() {
                return Err(Error::Disconnected);
            }
            if self.line.len() > MAX_EVENT || self.line.last() != Some(&b'\n') {
                return Err(Error::InvalidData);
            }
            #[derive(Deserialize)]
            struct Event {
                on: String,
            }
            let event: Event = decode(&self.line)?;
            self.line.clear();
            if matches!(event.on.as_str(), "sink" | "source" | "server" | "card") {
                return Ok(());
            }
        }
    }
    pub async fn close(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}
