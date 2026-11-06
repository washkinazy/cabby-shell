//! Memory-only capture/geometry; explicit saves publish a new private PNG atomically.
use image::{ImageDecoder, ImageEncoder};
use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    sync::{Semaphore, watch},
};
const MAX_ENCODED: usize = 33_554_432;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Timeout,
    Cancelled,
    InvalidImage,
    TooLarge,
    InvalidSelection,
    InvalidFilename,
    SaveFailed,
    FileExists,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "Screenshot capture unavailable",
            Self::Timeout => "Screenshot capture timed out",
            Self::Cancelled => "Screenshot cancelled",
            Self::InvalidImage => "Screenshot image is invalid",
            Self::TooLarge => "Screenshot exceeds image limits",
            Self::InvalidSelection => "Drag a larger region",
            Self::InvalidFilename => "Use a PNG filename without directories",
            Self::SaveFailed => "Could not save a new PNG in this directory",
            Self::FileExists => "That PNG filename already exists",
        })
    }
}
#[derive(Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
    pub png: Arc<Vec<u8>>,
}
impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
pub fn region(
    logical: (u32, u32),
    pixels: (u32, u32),
    a: (f32, f32),
    b: (f32, f32),
) -> Result<Rect, Error> {
    if logical.0 == 0
        || logical.1 == 0
        || pixels.0 == 0
        || pixels.1 == 0
        || ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite())
    {
        return Err(Error::InvalidSelection);
    }
    let axis = |a: f32, b: f32, logical: u32, pixels: u32| {
        let lo = (f64::from(a.min(b)).clamp(0., f64::from(logical)) * f64::from(pixels)
            / f64::from(logical))
        .floor() as u32;
        let hi = (f64::from(a.max(b)).clamp(0., f64::from(logical)) * f64::from(pixels)
            / f64::from(logical))
        .ceil() as u32;
        (lo, hi.min(pixels).saturating_sub(lo))
    };
    let (x, width) = axis(a.0, b.0, logical.0, pixels.0);
    let (y, height) = axis(a.1, b.1, logical.1, pixels.1);
    if width == 0 || height == 0 {
        return Err(Error::InvalidSelection);
    }
    Ok(Rect {
        x,
        y,
        width,
        height,
    })
}
fn decode(png: Vec<u8>) -> Result<Frame, Error> {
    if png.len() > MAX_ENCODED {
        return Err(Error::TooLarge);
    }
    let mut decoder =
        image::codecs::png::PngDecoder::new(Cursor::new(&png)).map_err(|_| Error::InvalidImage)?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u64::from(width) * u64::from(height) > 16_777_216
        || decoder.total_bytes() > 67_108_864
    {
        return Err(Error::TooLarge);
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(67_108_864);
    decoder.set_limits(limits).map_err(|_| Error::TooLarge)?;
    let rgba = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| Error::InvalidImage)?
        .to_rgba8()
        .into_raw();
    Ok(Frame {
        width,
        height,
        rgba: Arc::new(rgba),
        png: Arc::new(png),
    })
}
fn crop(frame: &Frame, r: Rect) -> Result<Frame, Error> {
    if r.width == 0
        || r.height == 0
        || r.x.checked_add(r.width).is_none_or(|n| n > frame.width)
        || r.y.checked_add(r.height).is_none_or(|n| n > frame.height)
    {
        return Err(Error::InvalidSelection);
    }
    let mut rgba = Vec::with_capacity(r.width as usize * r.height as usize * 4);
    for y in r.y..r.y + r.height {
        let start = ((y * frame.width + r.x) * 4) as usize;
        rgba.extend_from_slice(&frame.rgba[start..start + r.width as usize * 4]);
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&rgba, r.width, r.height, image::ExtendedColorType::Rgba8)
        .map_err(|_| Error::InvalidImage)?;
    if png.len() > MAX_ENCODED {
        return Err(Error::TooLarge);
    }
    Ok(Frame {
        width: r.width,
        height: r.height,
        rgba: Arc::new(rgba),
        png: Arc::new(png),
    })
}
async fn work<T: Send + 'static>(
    mut cancel: watch::Receiver<bool>,
    f: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    static WORK: OnceLock<Arc<Semaphore>> = OnceLock::new();
    if *cancel.borrow() {
        return Err(Error::Cancelled);
    }
    let permit = tokio::select! {p=WORK.get_or_init(||Arc::new(Semaphore::new(1))).clone().acquire_owned()=>p.map_err(|_|Error::Cancelled)?,_ = cancel.changed()=>return Err(Error::Cancelled)};
    let value = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        f()
    })
    .await
    .map_err(|_| Error::InvalidImage)??;
    if *cancel.borrow() {
        return Err(Error::Cancelled);
    }
    Ok(value)
}
pub async fn crop_region(
    frame: Frame,
    r: Rect,
    cancel: watch::Receiver<bool>,
) -> Result<Frame, Error> {
    work(cancel, move || crop(&frame, r)).await
}
pub async fn capture(
    output: String,
    cursor: bool,
    cancel: watch::Receiver<bool>,
) -> Result<Frame, Error> {
    capture_with(
        Path::new("grim"),
        output,
        cursor,
        cancel,
        Duration::from_secs(5),
    )
    .await
}
async fn capture_with(
    executable: &Path,
    output: String,
    cursor: bool,
    mut cancel: watch::Receiver<bool>,
    timeout: Duration,
) -> Result<Frame, Error> {
    use std::process::Stdio;
    if *cancel.borrow() {
        return Err(Error::Cancelled);
    }
    if output.is_empty() || output.len() > 256 || output.chars().any(char::is_control) {
        return Err(Error::Unavailable);
    }
    let mut command = tokio::process::Command::new(executable);
    use std::os::unix::process::CommandExt;
    let parent = unsafe { libc::getpid() };
    unsafe {
        command.as_std_mut().pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(std::io::Error::other("capture parent exited"));
            }
            Ok(())
        });
    }
    command.args(["-t", "png", "-o", &output]);
    if cursor {
        command.arg("-c");
    }
    let mut child = command
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| Error::Unavailable)?;
    let stdout = child.stdout.take().ok_or(Error::Unavailable)?;
    let result = {
        let read = async {
            let mut bytes = Vec::new();
            stdout
                .take((MAX_ENCODED + 1) as u64)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| Error::Unavailable)?;
            if bytes.len() > MAX_ENCODED {
                return Err(Error::TooLarge);
            }
            if !child
                .wait()
                .await
                .map_err(|_| Error::Unavailable)?
                .success()
            {
                return Err(Error::Unavailable);
            }
            Ok(bytes)
        };
        tokio::select! {r=read=>r,_ = cancel.changed()=>Err(Error::Cancelled),_ = tokio::time::sleep(timeout)=>Err(Error::Timeout)}
    };
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    let png = result?;
    work(cancel, move || decode(png)).await
}
/// Anonymous same-directory file: failed/cancelled/crashed writes leave no named capture.
pub fn save(
    frame: &Frame,
    directory: &Path,
    filename: &str,
    cancel: &AtomicBool,
) -> Result<PathBuf, Error> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };
    if filename.is_empty()
        || filename.len() > 255
        || filename.contains(['/', '\0'])
        || filename == "."
        || filename == ".."
        || !filename.to_ascii_lowercase().ends_with(".png")
    {
        return Err(Error::InvalidFilename);
    }
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    std::fs::create_dir_all(directory).map_err(|_| Error::SaveFailed)?;
    let path = CString::new(directory.as_os_str().as_bytes()).map_err(|_| Error::SaveFailed)?;
    let raw = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_TMPFILE | libc::O_RDWR | libc::O_CLOEXEC,
            0o600,
        )
    };
    if raw < 0 {
        return Err(Error::SaveFailed);
    }
    let mut file = unsafe { std::fs::File::from_raw_fd(raw) };
    for chunk in frame.png.chunks(65536) {
        if cancel.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        file.write_all(chunk).map_err(|_| Error::SaveFailed)?;
    }
    file.sync_all().map_err(|_| Error::SaveFailed)?;
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    let target = directory.join(filename);
    let target_c = CString::new(target.as_os_str().as_bytes()).map_err(|_| Error::SaveFailed)?;
    let source = CString::new(format!("/proc/self/fd/{}", file.as_raw_fd())).unwrap();
    if unsafe {
        libc::linkat(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            target_c.as_ptr(),
            libc::AT_SYMLINK_FOLLOW,
        )
    } != 0
    {
        return Err(
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
                Error::FileExists
            } else {
                Error::SaveFailed
            },
        );
    }
    Ok(target)
}
pub fn filename() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let time = now.as_secs() as libc::time_t;
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&time, &mut local) }.is_null() {
        return format!("Screenshot-{}-{}.png", now.as_secs(), now.subsec_nanos());
    }
    format!(
        "Screenshot-{:04}-{:02}-{:02}-{:02}{:02}{:02}-{:09}.png",
        local.tm_year + 1900,
        local.tm_mon + 1,
        local.tm_mday,
        local.tm_hour,
        local.tm_min,
        local.tm_sec,
        now.subsec_nanos()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fractional_reversed_clamped_and_oriented_geometry() {
        assert_eq!(
            region((100, 80), (150, 120), (40., 30.), (10., 10.)).unwrap(),
            Rect {
                x: 15,
                y: 15,
                width: 45,
                height: 30
            }
        );
        assert_eq!(
            region((80, 100), (120, 150), (-10., -20.), (100., 120.)).unwrap(),
            Rect {
                x: 0,
                y: 0,
                width: 120,
                height: 150
            }
        );
        assert!(region((100, 100), (150, 150), (4., 4.), (4., 4.)).is_err());
        assert!(region((100, 100), (150, 150), (f32::NAN, 1.), (5., 8.)).is_err());
        let frame =
            decode(include_bytes!("../../../examples/config/assets/sample.png").to_vec()).unwrap();
        let part = crop(
            &frame,
            Rect {
                x: 0,
                y: 0,
                width: 3,
                height: 2,
            },
        )
        .unwrap();
        assert_eq!((part.width, part.height), (3, 2));
        assert_eq!(&part.rgba[..12], &frame.rgba[..12]);
        assert_eq!(&part.rgba[12..24], &frame.rgba[64..76]);
        assert_eq!(decode(part.png.as_ref().clone()).unwrap().rgba, part.rgba);
        assert!(!format!("{frame:?}").contains("rgba"));
    }
    #[test]
    fn save_is_private_atomic_non_overwriting_and_cancellable() {
        use std::os::unix::fs::PermissionsExt;
        let frame =
            decode(include_bytes!("../../../examples/config/assets/sample.png").to_vec()).unwrap();
        let root = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let path = save(&frame, root.path(), "capture.png", &cancel).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), *frame.png);
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            save(&frame, root.path(), "capture.png", &cancel),
            Err(Error::FileExists)
        );
        assert_eq!(
            save(&frame, root.path(), "../wrong.png", &cancel),
            Err(Error::InvalidFilename)
        );
        cancel.store(true, Ordering::Release);
        assert_eq!(
            save(&frame, root.path(), "cancel.png", &cancel),
            Err(Error::Cancelled)
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        cancel.store(false, Ordering::Release);
        assert_eq!(
            save(
                &frame,
                &root.path().join("capture.png/dir"),
                "wrong.png",
                &cancel
            ),
            Err(Error::SaveFailed)
        );
    }
    #[tokio::test]
    async fn helper_timeout_cancellation_errors_bounds_and_literal_arguments_reap() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let helper = root.path().join("grim");
        let payload = root.path().join("payload");
        std::fs::write(
            payload,
            include_bytes!("../../../examples/config/assets/sample.png"),
        )
        .unwrap();
        std::fs::write(&helper,"#!/usr/bin/python3\nimport os,sys,time\nfrom pathlib import Path\np=Path(__file__).parent\n(p/'pid').write_text(str(os.getpid()))\n(p/'args').write_text(repr(sys.argv[1:]))\nmode=(p/'mode').read_text()\nif mode=='wait':time.sleep(20)\nif mode=='bad':sys.exit(1)\nif mode=='large':sys.stdout.buffer.write(b'x'*33554433)\nelse:sys.stdout.buffer.write((p/'payload').read_bytes())\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (tx, rx) = watch::channel(false);
        for mode in ["ok", "bad", "large", "wait"] {
            std::fs::write(root.path().join("mode"), mode).unwrap();
            let result = capture_with(
                &helper,
                "output;literal$(true)".into(),
                true,
                rx.clone(),
                Duration::from_millis(if mode == "wait" { 300 } else { 3000 }),
            )
            .await;
            match mode {
                "ok" => {
                    assert_eq!(result.unwrap().width, 16);
                }
                "bad" => assert!(matches!(result, Err(Error::Unavailable))),
                "large" => assert!(matches!(result, Err(Error::TooLarge))),
                _ => assert!(matches!(result, Err(Error::Timeout))),
            }
            let pid = std::fs::read_to_string(root.path().join("pid")).unwrap();
            assert!(!Path::new("/proc").join(pid).exists());
        }
        assert!(
            std::fs::read_to_string(root.path().join("args"))
                .unwrap()
                .contains("output;literal$(true)")
        );
        let task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            tx.send(true).unwrap();
        });
        assert!(matches!(
            capture_with(&helper, "output".into(), false, rx, Duration::from_secs(5)).await,
            Err(Error::Cancelled)
        ));
        task.await.unwrap();
        let pid = std::fs::read_to_string(root.path().join("pid")).unwrap();
        assert!(!Path::new("/proc").join(pid).exists());
    }
}
