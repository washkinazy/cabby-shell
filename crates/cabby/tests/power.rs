//! CLI boundary tests: every systemctl invocation is intercepted; no live power
//! or service operation can be reached, including on failed assertions.
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

const FIXTURE: &str = r#"#!/usr/bin/python3
import json, os, sys
from pathlib import Path
root = Path(os.environ['CABBY_POWER_FIXTURE'])
args = sys.argv[1:]
with (root/'calls').open('a') as log: log.write(json.dumps(args)+'\n')
mode = os.environ['CABBY_POWER_CASE']
assert args.pop(0) == '--no-ask-password'
if args == ['--check-inhibitors=yes', '--no-block', 'suspend']:
    assert (root/'ready').exists()
    (root/'suspend').touch()
    sys.exit(1 if mode == 'suspend-denied' else 0)
assert args.pop(0) == '--user'
if args == ['reset-failed', 'fixture-lock.service']:
    sys.exit(1 if mode == 'unloaded-unit' else 0)
if args == ['start', 'fixture-lock.service']:
    if mode in ('lock-denied', 'no-ready'): sys.exit(1)
    (root/'ready').touch()
    sys.exit(0)
if args == ['is-active', '--quiet', 'fixture-session.service']:
    sys.exit(1 if mode == 'no-session' or (mode == 'session-lost' and (root/'ready').exists()) else 0)
if args == ['is-active', '--quiet', 'fixture-lock.service']:
    sys.exit(1 if mode == 'locker-exited' else 0)
raise AssertionError(args)
"#;

#[test]
fn suspend_is_gated_on_readiness_and_rechecks_and_preserves_inhibitors() {
    for mode in [
        "success",
        "unloaded-unit",
        "no-session",
        "lock-denied",
        "no-ready",
        "session-lost",
        "locker-exited",
        "suspend-denied",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("systemctl");
        fs::write(&executable, FIXTURE).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_cabby"))
            .args([
                "suspend",
                "--session-unit",
                "fixture-session.service",
                "--lock-unit",
                "fixture-lock.service",
            ])
            .env("PATH", temp.path())
            .env("CABBY_POWER_FIXTURE", temp.path())
            .env("CABBY_POWER_CASE", mode)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            matches!(mode, "success" | "unloaded-unit"),
            "{mode}: {output:?}"
        );
        assert_eq!(
            temp.path().join("suspend").exists(),
            matches!(mode, "success" | "unloaded-unit" | "suspend-denied"),
            "{mode}"
        );
        let calls = fs::read_to_string(temp.path().join("calls")).unwrap();
        let calls: Vec<Vec<String>> = calls
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(!calls.iter().flatten().any(|arg| matches!(
            arg.as_str(),
            "stop" | "restart" | "--force" | "--check-inhibitors=no"
        )));
        if mode == "success" {
            assert_eq!(calls.len(), 6);
            assert_eq!(calls[2][2], "start");
            assert_eq!(calls[3].last().unwrap(), "fixture-session.service");
            assert_eq!(calls[4].last().unwrap(), "fixture-lock.service");
            assert_eq!(calls[5].last().unwrap(), "suspend");
        }
    }
}

#[test]
fn invalid_suspend_options_fail_before_any_helper() {
    for args in [
        vec!["suspend", "--lock-unit", "../invalid.service"],
        vec!["suspend", "--target", "cabby.target"],
        vec![
            "suspend",
            "--lock-unit",
            "first.service",
            "--lock-unit",
            "second.service",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_cabby"))
            .args(args)
            .env("PATH", "/nonexistent")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("No such file"));
    }
}

#[test]
fn display_power_requires_lock_and_restoration_never_unlocks() {
    for denied in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let ctl = temp.path().join("systemctl");
        fs::write(
            &ctl,
            format!(
                "#!/usr/bin/python3\nimport sys\nsys.exit({} if 'start' in sys.argv else 0)\n",
                u8::from(denied)
            ),
        )
        .unwrap();
        fs::set_permissions(&ctl, fs::Permissions::from_mode(0o700)).unwrap();
        let niri = temp.path().join("niri");
        fs::write(&niri, "#!/usr/bin/python3\nimport os,sys\nfrom pathlib import Path\nassert sys.argv[1:3]==['msg','action']\nassert sys.argv[3] in ['power-off-monitors','power-on-monitors']\nwith (Path(os.environ['CABBY_POWER_FIXTURE'])/'power').open('a') as f: f.write(sys.argv[3]+'\\n')\n").unwrap();
        fs::set_permissions(&niri, fs::Permissions::from_mode(0o700)).unwrap();
        let run = |action| {
            Command::new(env!("CARGO_BIN_EXE_cabby"))
                .arg(action)
                .env("PATH", temp.path())
                .env("CABBY_POWER_FIXTURE", temp.path())
                .output()
                .unwrap()
        };
        assert_eq!(run("display-off").status.success(), !denied);
        assert_eq!(temp.path().join("power").exists(), !denied);
        assert!(run("display-on").status.success());
        let calls = fs::read_to_string(temp.path().join("power")).unwrap();
        assert_eq!(
            calls,
            if denied {
                "power-on-monitors\n"
            } else {
                "power-off-monitors\npower-on-monitors\n"
            }
        );
    }
}
