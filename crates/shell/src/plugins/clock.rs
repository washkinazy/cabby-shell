//! Local date/time, updated at minute boundaries instead of a frame/second timer.
use std::time::{Duration, SystemTime, UNIX_EPOCH};
pub fn now() -> String {
    let Ok(elapsed) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return "Clock unavailable".into();
    };
    let seconds = elapsed.as_secs() as libc::time_t;
    let mut time = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: both pointers refer to live storage; localtime_r initializes tm on success.
    if unsafe { libc::localtime_r(&seconds, time.as_mut_ptr()) }.is_null() {
        return "Clock unavailable".into();
    }
    let time = unsafe { time.assume_init() };
    format_local(&time)
}
fn format_local(time: &libc::tm) -> String {
    let mut text = [0u8; 64];
    // SAFETY: a valid tm, a NUL-terminated format and an output buffer of the stated size.
    let len = unsafe {
        libc::strftime(
            text.as_mut_ptr().cast(),
            text.len(),
            c"%a %d %b  %H:%M".as_ptr(),
            time,
        )
    };
    String::from_utf8_lossy(&text[..len]).into_owned()
}
pub fn until_next_minute(elapsed: Duration) -> Duration {
    Duration::from_secs(60 - elapsed.as_secs() % 60)
        .saturating_sub(Duration::from_nanos(u64::from(elapsed.subsec_nanos())))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_formats_date_and_time_and_aligns_to_minutes() {
        // SAFETY: all-zero tm is valid storage; populate the fields used by strftime.
        let mut time: libc::tm = unsafe { std::mem::zeroed() };
        time.tm_wday = 4;
        time.tm_mday = 5;
        time.tm_mon = 10;
        time.tm_year = 126;
        time.tm_hour = 9;
        time.tm_min = 7;
        assert_eq!(format_local(&time), "Thu 05 Nov  09:07");
        assert_eq!(
            until_next_minute(Duration::from_millis(61_250)),
            Duration::from_millis(58_750)
        );
        assert_eq!(
            until_next_minute(Duration::from_secs(120)),
            Duration::from_secs(60)
        );
    }
}
