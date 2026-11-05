use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn status(runtime: &Path) -> Option<serde_json::Value> {
    let mut socket = UnixStream::connect(runtime.join("control.sock")).ok()?;
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    writeln!(socket, "status").ok()?;
    let mut line = String::new();
    BufReader::new(socket).read_line(&mut line).ok()?;
    serde_json::from_str(&line).ok()
}
fn wait(runtime: &Path, height: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if status(runtime).is_some_and(|s| s["config"]["bar_height"] == height) {
            return;
        }
        assert!(Instant::now() < deadline, "automatic reload timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn atomic_saves_invalid_edits_directory_recreation_and_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let configdir = temp.path().join("config");
    fs::create_dir(&configdir).unwrap();
    let config = configdir.join("config.toml");
    let runtime = temp.path().join("runtime");
    fs::write(&config, "bar_height=40").unwrap();
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_cabby"))
            .args(["serve", "--config"])
            .arg(&config)
            .arg("--runtime-dir")
            .arg(&runtime)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait(&runtime, 40);
    fs::write(&config, "bar_height=48").unwrap();
    wait(&runtime, 48);
    fs::write(configdir.join("replacement"), "bar_height=56").unwrap();
    fs::rename(configdir.join("replacement"), &config).unwrap();
    wait(&runtime, 56);
    fs::write(&config, "invalid=[").unwrap();
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(status(&runtime).unwrap()["config"]["bar_height"], 56);
    fs::remove_dir_all(&configdir).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    fs::create_dir(&configdir).unwrap();
    fs::write(&config, "bar_height=64").unwrap();
    wait(&runtime, 64);
    let target = temp.path().join("linked.toml");
    fs::write(&target, "bar_height=72").unwrap();
    fs::remove_file(&config).unwrap();
    std::os::unix::fs::symlink(&target, &config).unwrap();
    wait(&runtime, 72);
    fs::write(&target, "bar_height=80").unwrap();
    wait(&runtime, 80);
}
