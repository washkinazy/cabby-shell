//! Private one-attempt helper. No credential arguments, environment override, or test mode.
fn main() {
    if std::env::args_os().len() != 1 || cabby_lock::disable_dumps().is_err() {
        std::process::exit(2);
    }
    let secret = match cabby_lock::Secret::read(&mut std::io::stdin().lock()) {
        Ok(s) => s,
        Err(_) => std::process::exit(2),
    };
    let result = cabby_lock::authenticate_current(&secret);
    drop(secret);
    std::process::exit(match result {
        Ok(()) => 0,
        Err(cabby_lock::auth::Error::Denied) => 1,
        Err(_) => 2,
    });
}
