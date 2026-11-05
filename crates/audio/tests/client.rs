use cabby_audio::{Client, Command, Error, Kind, Operation, parse};
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

const INFO: &[u8] = br#"{"default_sink_name":"speaker","default_source_name":"mic"}"#;
const SINKS: &[u8] = br#"[{"name":"speaker","description":"Speakers","mute":false,"volume":{"left":{"value":32768},"right":{"value":65536}},"monitor_source":"speaker.monitor"}]"#;
const SOURCES: &[u8] = br#"[{"name":"mic","description":"Microphone","mute":true,"volume":{"mono":{"value":16384}},"monitor_source":null},{"name":"speaker.monitor","description":"Monitor","mute":false,"volume":{"mono":{"value":65536}},"monitor_source":"speaker"}]"#;

#[test]
fn snapshot_controls_and_bounds() {
    let snapshot = parse(INFO, SINKS, SOURCES).unwrap();
    assert_eq!(snapshot.devices.len(), 2);
    assert_eq!(snapshot.devices[0].percent, 100); // Max channel, not an average.
    assert_eq!(snapshot.devices[1].percent, 25);
    assert!(snapshot.is_default(&snapshot.devices[1]));
    for (op, expected) in [
        (
            Operation::Volume(0),
            vec!["set-sink-volume", "speaker", "0%"],
        ),
        (Operation::Mute(true), vec!["set-sink-mute", "speaker", "1"]),
        (Operation::Default, vec!["set-default-sink", "speaker"]),
    ] {
        assert_eq!(
            snapshot
                .arguments(&Command {
                    kind: Kind::Output,
                    name: "speaker".into(),
                    operation: op
                })
                .unwrap(),
            expected
        );
    }
    for (kind, name, operation) in [
        (Kind::Input, "speaker", Operation::Default),
        (Kind::Output, "missing", Operation::Mute(true)),
        (Kind::Output, "speaker", Operation::Volume(101)),
    ] {
        assert_eq!(
            snapshot.arguments(&Command {
                kind,
                name: name.into(),
                operation
            }),
            Err(Error::InvalidCommand)
        );
    }
    assert!(parse(b"{}", b"[]", b"[]").unwrap().devices.is_empty());
    assert!(
        parse(
            INFO,
            br#"[{"name":"--bad","mute":false,"volume":{"mono":{"value":1}}}]"#,
            b"[]"
        )
        .is_err()
    );
    assert!(
        parse(
            INFO,
            br#"[{"name":"speaker","mute":false,"volume":{}}]"#,
            b"[]"
        )
        .is_err()
    );
    assert!(parse(INFO, &[b' '; 1024 * 1024 + 1], b"[]").is_err());
    assert!(!format!("{snapshot:?}").contains("speaker"));
}

fn fixture(mode: &str) -> (tempfile::TempDir, Client) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("info"), INFO).unwrap();
    fs::write(dir.path().join("sinks"), SINKS).unwrap();
    fs::write(dir.path().join("sources"), SOURCES).unwrap();
    let script = format!(
        r#"#!/usr/bin/python3
import sys,json,time,os
from pathlib import Path
root=Path({root})
mode={mode}
with (root/'pids').open('a') as f: f.write(str(os.getpid())+'\n')
args=sys.argv[1:]
with (root/'args').open('a') as f: f.write(json.dumps(args)+'\n')
if mode=='hang': time.sleep(30)
if mode=='large': print('x'*1100000); sys.exit(0)
if mode=='reject': sys.exit(1)
if args==['--format=json','subscribe']:
    if mode=='partial':
        print('{{"on":',end='',flush=True)
        time.sleep(.2)
        print('"sink"}}',flush=True)
    elif mode=='bad': print('x'*5000,flush=True)
    else:
        print('{{"on":"client"}}',flush=True)
        print('{{"on":"sink-input"}}',flush=True)
        print('{{"on":"source"}}',flush=True)
    time.sleep(30)
elif args==['--format=json','info']: print((root/'info').read_text())
elif args[:2]==['--format=json','list']: print((root/args[2]).read_text())
"#,
        root = serde_json::to_string(dir.path().to_str().unwrap()).unwrap(),
        mode = serde_json::to_string(mode).unwrap()
    );
    let path = dir.path().join("pactl");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    (dir, Client::with_executable(path))
}
async fn gone(dir: &tempfile::TempDir) {
    let pids = fs::read_to_string(dir.path().join("pids")).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while pids
            .lines()
            .any(|pid| std::path::Path::new("/proc").join(pid).exists())
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("helper was not reaped");
}
#[tokio::test]
async fn helpers_use_literal_arguments_and_events_are_owned() {
    let (dir, client) = fixture("normal");
    let snapshot = client.snapshot().await.unwrap();
    client
        .apply(
            &snapshot,
            &Command {
                kind: Kind::Input,
                name: "mic".into(),
                operation: Operation::Mute(false),
            },
        )
        .await
        .unwrap();
    assert!(
        fs::read_to_string(dir.path().join("args"))
            .unwrap()
            .contains("[\"set-source-mute\", \"mic\", \"0\"]")
    );
    let mut events = client.subscribe().unwrap();
    tokio::time::timeout(Duration::from_secs(2), events.next())
        .await
        .unwrap()
        .unwrap();
    events.close().await;
    gone(&dir).await;
}
#[tokio::test]
async fn partial_event_survives_cancellation_and_drop_reaps_subscription() {
    let (dir, client) = fixture("partial");
    let mut events = client.subscribe().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), events.next())
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(2), events.next())
        .await
        .unwrap()
        .unwrap();
    drop(events);
    gone(&dir).await;
}
#[tokio::test]
async fn errors_timeouts_oversize_and_cancellation_leave_no_helpers() {
    for (mode, error) in [
        ("hang", Error::Timeout),
        ("large", Error::InvalidData),
        ("reject", Error::Rejected),
    ] {
        let (dir, client) = fixture(mode);
        assert_eq!(client.snapshot().await.unwrap_err(), error);
        gone(&dir).await;
    }
    let (dir, client) = fixture("hang");
    let task = tokio::spawn(async move { client.snapshot().await });
    while !dir.path().join("pids").exists() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    task.abort();
    let _ = task.await;
    gone(&dir).await;
    let (dir, client) = fixture("bad");
    let mut events = client.subscribe().unwrap();
    assert_eq!(events.next().await, Err(Error::InvalidData));
    events.close().await;
    gone(&dir).await;
    assert_eq!(
        Client::with_executable(dir.path().join("absent"))
            .snapshot()
            .await
            .unwrap_err(),
        Error::Unavailable
    );
}
