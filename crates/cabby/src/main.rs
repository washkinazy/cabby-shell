mod runtime;
use cabby_config::{Result, Source};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next().unwrap_or_default();
    if command == "--help" || command == "help" {
        println!(
            "Usage: cabby validate|serve|status|reload|ui [--config PATH] [--runtime-dir PATH]\nserve additionally accepts --ui-unit NAME"
        );
        return Ok(());
    }
    let mut config = None;
    let mut directory = None;
    let mut ui_unit = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or("option requires a value")?;
        match flag.to_str() {
            Some("--config") if config.is_none() => config = Some(PathBuf::from(value)),
            Some("--runtime-dir") if directory.is_none() => directory = Some(PathBuf::from(value)),
            Some("--ui-unit") if ui_unit.is_none() => {
                ui_unit = Some(value.into_string().map_err(|_| "unit must be UTF-8")?)
            }
            _ => return Err("unknown or duplicate option".into()),
        }
    }
    match command.to_str() {
        Some("validate") => {
            let source = Source::from_env(config.as_deref())?;
            println!(
                "Valid configuration: {}\n{}",
                source.path.display(),
                source.load(true)?.to_toml()
            );
        }
        Some("serve") => runtime::serve(
            Source::from_env(config.as_deref())?,
            runtime::directory(directory)?,
            ui_unit,
        )?,
        Some("status" | "reload") => {
            let result =
                runtime::request(&runtime::directory(directory)?, command.to_str().unwrap())?;
            println!("{result}");
            if result["ok"] != true {
                return Err("request failed".into());
            }
        }
        Some("ui") => {
            let source = Source::from_env(config.as_deref())?;
            source.load(true)?;
            let executable = std::env::current_exe()
                .map_err(|error| error.to_string())?
                .with_file_name("cabby-platform-spike");
            let error = std::process::Command::new(executable)
                .arg("--config")
                .arg(source.path)
                .exec();
            return Err(format!("cannot start UI: {error}"));
        }
        _ => return Err("unknown command; use cabby --help".into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cabby: {error}");
        std::process::exit(1);
    }
}
