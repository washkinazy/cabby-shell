use crate::Secret;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Denied,
    Unavailable,
    Timeout,
}
/// The process is owned by the caller's future and service cgroup, never detached.
#[derive(Clone)]
pub struct Client {
    executable: PathBuf,
    timeout: Duration,
}
impl Client {
    pub fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            timeout: Duration::from_secs(30),
        }
    }
    pub async fn authenticate(&self, secret: Secret) -> Result<(), Error> {
        if secret.is_empty() {
            return Err(Error::Denied);
        }
        let mut child = tokio::process::Command::new(&self.executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| Error::Unavailable)?;
        let result = tokio::time::timeout(self.timeout, async {
            let mut input = child.stdin.take().ok_or(Error::Unavailable)?;
            input
                .write_all(&(secret.as_bytes().len() as u32).to_be_bytes())
                .await
                .map_err(|_| Error::Unavailable)?;
            input
                .write_all(secret.as_bytes())
                .await
                .map_err(|_| Error::Unavailable)?;
            drop(input);
            drop(secret);
            match child.wait().await.map_err(|_| Error::Unavailable)?.code() {
                Some(0) => Ok(()),
                Some(1) => Err(Error::Denied),
                _ => Err(Error::Unavailable),
            }
        })
        .await
        .unwrap_or(Err(Error::Timeout));
        if result.is_err() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[tokio::test]
    async fn helper_protocol_failures_timeout_and_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("auth.py");
        std::fs::write(&helper,"#!/usr/bin/python3\nimport sys,struct,time,os\nfrom pathlib import Path\nPath(__file__).with_suffix('.pid').write_text(str(os.getpid()))\nn=struct.unpack('!I',sys.stdin.buffer.read(4))[0]\nvalue=sys.stdin.buffer.read(n)\nif value==b'hang': time.sleep(60)\nsys.exit(0 if value==b'fixture' else 1)\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let client = Client {
            executable: helper.clone(),
            timeout: Duration::from_millis(200),
        };
        for (text, result) in [
            ("fixture", Ok(())),
            ("wrong", Err(Error::Denied)),
            ("hang", Err(Error::Timeout)),
        ] {
            let mut secret = Secret::default();
            secret.append(text);
            assert_eq!(client.authenticate(secret).await, result);
        }
        let mut secret = Secret::default();
        secret.append("hang");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), client.authenticate(secret))
                .await
                .is_err()
        );
        let pid = std::fs::read_to_string(helper.with_extension("pid")).unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while std::path::Path::new(&format!("/proc/{pid}")).exists() {
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
