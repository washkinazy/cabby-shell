use cabby_config::{Result, Source};
use std::path::PathBuf;

fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next().unwrap_or_default();
    if command == "--help" || command == "help" {
        println!("Usage: cabby validate [--config PATH]");
        return Ok(());
    }
    if command != "validate" {
        return Err("Usage: cabby validate [--config PATH]".into());
    }
    let path = match args.next() {
        None => None,
        Some(flag) if flag == "--config" => Some(PathBuf::from(
            args.next().ok_or("--config requires a path")?,
        )),
        _ => return Err("expected --config PATH".into()),
    };
    if args.next().is_some() {
        return Err("unexpected arguments".into());
    }
    let source = Source::from_env(path.as_deref())?;
    let config = source.load(true)?;
    println!(
        "Valid configuration: {}\n{}",
        source.path.display(),
        config.to_toml()
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cabby: {error}");
        std::process::exit(1);
    }
}
