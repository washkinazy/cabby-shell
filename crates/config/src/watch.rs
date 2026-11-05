//! Linux directory watches survive atomic file replacement; no config polling.
use std::{
    collections::HashMap,
    ffi::{CString, OsString},
    fs::File,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::{OsStrExt, OsStringExt},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub struct Watch {
    file: File,
    directories: HashMap<i32, PathBuf>,
    targets: Vec<PathBuf>,
}
impl Watch {
    pub fn new(paths: impl IntoIterator<Item = PathBuf>) -> io::Result<Self> {
        // SAFETY: flags are valid and the returned owned descriptor is checked.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let mut targets = Vec::new();
        for path in paths {
            if let Ok(real) = path.canonicalize()
                && real != path
            {
                targets.push(real);
            }
            targets.push(path);
        }
        let mut directories = HashMap::new();
        for target in &targets {
            for directory in target.ancestors().skip(1) {
                if directories.values().any(|p| p == directory) {
                    continue;
                }
                let name = CString::new(directory.as_os_str().as_bytes())?;
                // SAFETY: name is NUL-terminated and file owns the live descriptor.
                let wd = unsafe {
                    libc::inotify_add_watch(
                        fd,
                        name.as_ptr(),
                        libc::IN_CLOSE_WRITE
                            | libc::IN_MOVED_TO
                            | libc::IN_CREATE
                            | libc::IN_DELETE
                            | libc::IN_MOVED_FROM
                            | libc::IN_ATTRIB
                            | libc::IN_DELETE_SELF
                            | libc::IN_MOVE_SELF,
                    )
                };
                if wd >= 0 {
                    directories.insert(wd, directory.to_path_buf());
                } else if !matches!(
                    io::Error::last_os_error().raw_os_error(),
                    Some(libc::ENOENT | libc::ENOTDIR)
                ) {
                    return Err(io::Error::last_os_error());
                }
            }
        }
        Ok(Self {
            file,
            directories,
            targets,
        })
    }
    pub fn refresh(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> io::Result<bool> {
        let next = Self::new(paths)?;
        let raced = self.targets != next.targets || self.drain()?;
        *self = next;
        Ok(raced)
    }
    pub fn drain(&mut self) -> io::Result<bool> {
        let mut relevant = false;
        let mut buffer = [0u8; 16384];
        loop {
            let len = match self.file.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            };
            let mut offset = 0;
            while offset + std::mem::size_of::<libc::inotify_event>() <= len {
                // SAFETY: enough initialized bytes remain; unaligned reads are intentional.
                let event = unsafe {
                    std::ptr::read_unaligned(
                        buffer[offset..].as_ptr().cast::<libc::inotify_event>(),
                    )
                };
                let start = offset + std::mem::size_of::<libc::inotify_event>();
                let end = start + event.len as usize;
                if end > len {
                    return Err(io::Error::other("truncated inotify event"));
                }
                if event.mask & libc::IN_Q_OVERFLOW != 0 {
                    relevant = true;
                }
                if let Some(directory) = self.directories.get(&event.wd) {
                    let name = &buffer[start..end];
                    let name = &name[..name.iter().position(|b| *b == 0).unwrap_or(name.len())];
                    let path = directory.join(OsString::from_vec(name.to_vec()));
                    if self
                        .targets
                        .iter()
                        .any(|t| t == &path || t.starts_with(&path))
                    {
                        relevant = true;
                    }
                }
                offset = end;
            }
        }
        Ok(relevant)
    }
    pub fn wait(&self, listener: i32, deadline: Option<Instant>) -> io::Result<bool> {
        let mut fds = [
            libc::pollfd {
                fd: self.file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: listener,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let timeout = deadline
            .map(|d| {
                d.saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(i32::MAX as u128) as i32
            })
            .unwrap_or(-1);
        // SAFETY: two initialized pollfd entries, valid array length, bounded timeout.
        if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) } < 0 {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
        Ok(fds[1].revents & libc::POLLIN != 0)
    }
}
pub fn paths(config: &Path, wallpaper: &str) -> Vec<PathBuf> {
    let mut paths = vec![config.to_path_buf()];
    if !wallpaper.is_empty() {
        paths.push(PathBuf::from(wallpaper));
    }
    paths
}
pub const DEBOUNCE: Duration = Duration::from_millis(300);

impl AsRawFd for Watch {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.file.as_raw_fd()
    }
}
