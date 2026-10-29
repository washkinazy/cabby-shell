mod runtime;
use cabby_config::{Result, Source};
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

const HELP: &str = "Usage:
  cabby validate [--config PATH]
  cabby status|reload [--runtime-dir PATH]
  cabby start|stop|restart [--target UNIT] [--session-unit UNIT]
  cabby restart-ui [--ui-unit UNIT] [--session-unit UNIT]
  cabby launch [--session-unit UNIT] -- PROGRAM [ARG...]
  cabby serve [--config PATH] [--runtime-dir PATH] [--ui-unit UNIT]
  cabby ui [--config PATH]

Defaults: target=cabby.target, session-unit=niri.service, ui-unit=cabby-ui.service.
No command installs or enables services. Paths and arguments are passed without shell evaluation.";

fn unit(value: &str, suffix: &str) -> Result<()> {
    if value.len() > 255
        || !value.ends_with(suffix)
        || value.starts_with('-')
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-@".contains(&b))
    {
        return Err(format!("invalid unit name: {value}"));
    }
    Ok(())
}

fn systemctl(args: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("systemd command failed".into());
    }
    Ok(())
}

fn require_session(session: &str) -> Result<()> {
    let status = Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", session])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!(
            "session {session} is not active; refusing to start a compositor"
        ));
    }
    Ok(())
}

fn launch(session: &str, program: &[OsString]) -> Result<()> {
    if program.is_empty() {
        return Err("launch requires -- PROGRAM [ARG...]".into());
    }
    require_session(session)?;
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let name = format!("app-cabby-{}-{time}.service", std::process::id());
    let mut cmd = Command::new("systemd-run");
    cmd.args([
        "--user",
        "--collect",
        "--quiet",
        "--service-type=exec",
        "--expand-environment=no",
        "--same-dir",
    ])
    .arg(format!("--unit={name}"))
    .arg(format!("--property=BindsTo={session}"))
    .arg(format!("--property=After={session}"))
    .args([
        "--property=KillMode=control-group",
        "--property=TimeoutStopSec=5",
    ]);
    for key in [
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "XAUTHORITY",
        "XDG_RUNTIME_DIR",
        "XDG_CURRENT_DESKTOP",
        "NIRI_SOCKET",
    ] {
        if let Some(value) = std::env::var_os(key) {
            let mut argument = OsString::from(format!("--setenv={key}="));
            argument.push(value);
            cmd.arg(argument);
        }
    }
    let status = cmd
        .arg("--")
        .args(program)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("application launch failed".into());
    }
    println!("{}", serde_json::json!({"ok": true, "unit": name}));
    Ok(())
}

fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next().unwrap_or_default();
    if command == "--help" || command == "help" {
        println!("{HELP}");
        return Ok(());
    }
    let command = command.to_str().ok_or("command must be UTF-8")?;
    let allowed: &[&str] = match command {
        "validate" | "ui" => &["--config"],
        "serve" => &["--config", "--runtime-dir", "--ui-unit"],
        "status" | "reload" => &["--runtime-dir"],
        "start" | "stop" | "restart" => &["--target", "--session-unit"],
        "restart-ui" => &["--ui-unit", "--session-unit"],
        "launch" => &["--session-unit"],
        _ => return Err("unknown command; use cabby --help".into()),
    };
    let mut options = std::collections::BTreeMap::new();
    let mut program = Vec::new();
    while let Some(flag) = args.next() {
        if flag == "--" && command == "launch" {
            program.extend(args);
            break;
        }
        let flag = flag.into_string().map_err(|_| "option must be UTF-8")?;
        if !allowed.contains(&flag.as_str()) || options.contains_key(&flag) {
            return Err(format!("unknown or duplicate option for {command}: {flag}"));
        }
        options.insert(flag, args.next().ok_or("option requires a value")?);
    }
    let config = options.get("--config").map(PathBuf::from);
    let directory = options.get("--runtime-dir").map(PathBuf::from);
    let text_option = |key: &str, default: &str| -> Result<String> {
        options
            .get(key)
            .map(|v| {
                v.to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("{key} must be UTF-8"))
            })
            .unwrap_or_else(|| Ok(default.to_owned()))
    };
    let session = text_option("--session-unit", "niri.service")?;
    let target = text_option("--target", "cabby.target")?;
    let ui = text_option("--ui-unit", "cabby-ui.service")?;
    unit(&session, ".service")?;
    unit(&target, ".target")?;
    unit(&ui, ".service")?;
    match command {
        "validate" => {
            let source = Source::from_env(config.as_deref())?;
            println!(
                "Valid configuration: {}\n{}",
                source.path.display(),
                source.load(true)?.to_toml()
            );
        }
        "serve" => runtime::serve(
            Source::from_env(config.as_deref())?,
            runtime::directory(directory)?,
            options.contains_key("--ui-unit").then_some(ui),
        )?,
        "status" | "reload" => {
            let result = runtime::request(&runtime::directory(directory)?, command)?;
            println!("{result}");
            if result["ok"] != true {
                return Err("request failed".into());
            }
            if result["ui_restart_queued"] == false {
                return Err("configuration accepted, but UI restart could not be queued".into());
            }
        }
        "ui" => {
            let source = Source::from_env(config.as_deref())?;
            source.load(true)?;
            let executable = std::env::current_exe()
                .map_err(|error| error.to_string())?
                .with_file_name("cabby-platform-spike");
            let mut child = Command::new(executable);
            if source.explicit {
                child.arg("--config").arg(source.path);
            }
            return Err(format!("cannot start UI: {}", child.exec()));
        }
        "start" | "restart" => {
            require_session(&session)?;
            systemctl(&[command, &target])?;
        }
        "stop" => systemctl(&["stop", &target])?,
        "restart-ui" => {
            require_session(&session)?;
            systemctl(&["restart", &ui])?;
        }
        "launch" => launch(&session, &program)?,
        _ => unreachable!(),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cabby: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unit_names_cannot_be_options_or_paths() {
        for value in [
            "--help.service",
            "../other.service",
            "a b.service",
            "x;touch.service",
            "a.target",
        ] {
            assert!(unit(value, ".service").is_err());
        }
        assert!(unit("cabby-test-123.service", ".service").is_ok());
    }
}
