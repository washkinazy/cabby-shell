//! Bounded sealed-memory image transfer over the existing private control socket.
use std::{
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::net::UnixStream,
    },
};
pub fn receive_request(stream: &mut UnixStream) -> std::io::Result<(String, Option<OwnedFd>)> {
    let mut data = [0u8; 4097];
    let mut control = [0usize; 8];
    let mut io = libc::iovec {
        iov_base: data.as_mut_ptr().cast(),
        iov_len: data.len(),
    };
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut io;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = std::mem::size_of_val(&control);
    let n = unsafe { libc::recvmsg(stream.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC) };
    if n <= 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut fds = Vec::new();
    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let len = (*cmsg).cmsg_len.saturating_sub(libc::CMSG_LEN(0) as usize)
                    / std::mem::size_of::<i32>();
                for i in 0..len {
                    fds.push(OwnedFd::from_raw_fd(std::ptr::read_unaligned(
                        libc::CMSG_DATA(cmsg).cast::<i32>().add(i),
                    )));
                }
            }
            cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
        }
    }
    if msg.msg_flags & libc::MSG_CTRUNC != 0 || fds.len() > 1 {
        return Err(std::io::Error::other("invalid request descriptors"));
    }
    let mut bytes = data[..n as usize].to_vec();
    while !bytes.contains(&b'\n') && bytes.len() <= 4096 {
        let n = stream.read(&mut data[..(4097 - bytes.len()).min(4097)])?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&data[..n]);
    }
    if bytes.len() > 4096
        || bytes.last() != Some(&b'\n')
        || bytes[..bytes.len() - 1].contains(&b'\n')
    {
        return Err(std::io::Error::other("invalid request length"));
    }
    let command =
        String::from_utf8(bytes).map_err(|_| std::io::Error::other("invalid request encoding"))?;
    Ok((command, fds.pop()))
}
pub fn copy_png(root: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::BufRead;
    if bytes.len() > 16_777_216 {
        return Err("Image exceeds the clipboard maximum of 16 MiB".into());
    }
    let raw = unsafe {
        libc::memfd_create(
            c"cabby-image".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    if raw < 0 {
        return Err("Clipboard image transport unavailable".into());
    }
    let mut file = unsafe { std::fs::File::from_raw_fd(raw) };
    file.write_all(bytes)
        .map_err(|_| "Clipboard image transfer failed")?;
    if unsafe {
        libc::fcntl(
            raw,
            libc::F_ADD_SEALS,
            libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE,
        )
    } < 0
    {
        return Err("Clipboard image transport unavailable".into());
    }
    let stream = UnixStream::connect(root.join("control.sock"))
        .map_err(|_| "Clipboard coordinator unavailable")?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(4)))
        .map_err(|_| "Clipboard unavailable")?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(2)))
        .map_err(|_| "Clipboard unavailable")?;
    let data = b"{\"clipboard\":\"import-png\"}\n";
    let mut io = libc::iovec {
        iov_base: data.as_ptr().cast_mut().cast(),
        iov_len: data.len(),
    };
    let mut control = [0usize; 8];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut io;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = unsafe { libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32) } as usize;
    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<i32>() as u32) as usize;
        std::ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<i32>(), raw);
    }
    let sent = unsafe { libc::sendmsg(stream.as_raw_fd(), &msg, libc::MSG_NOSIGNAL) };
    if sent != data.len() as isize {
        return Err("Clipboard image transfer failed".into());
    }
    let mut response = String::new();
    std::io::BufReader::new(stream.take(4097))
        .read_line(&mut response)
        .map_err(|_| "Clipboard image transfer failed")?;
    let value: serde_json::Value =
        serde_json::from_str(&response).map_err(|_| "Clipboard image transfer failed")?;
    if value["ok"] != true {
        return Err(value["error"]
            .as_str()
            .unwrap_or("Clipboard image selection failed")
            .to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sealed_png_descriptors_are_cloexec_and_normal_requests_can_fragment() {
        let root = tempfile::tempdir().unwrap();
        let listener =
            std::os::unix::net::UnixListener::bind(root.path().join("control.sock")).unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let (command, fd) = receive_request(&mut stream).unwrap();
            assert!(command.contains("import-png"));
            let fd = fd.unwrap();
            assert_ne!(
                unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
                0
            );
            let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GET_SEALS) };
            assert_ne!(flags & libc::F_SEAL_WRITE, 0);
            let file = std::fs::File::from(fd);
            let mut content = Vec::new();
            use std::os::unix::fs::FileExt;
            let len = file.metadata().unwrap().len();
            content.resize(len as usize, 0);
            file.read_exact_at(&mut content, 0).unwrap();
            assert_eq!(
                content,
                include_bytes!("../../../examples/config/assets/sample.png")
            );
            stream.write_all(b"{\"ok\":true}\n").unwrap();
        });
        copy_png(
            root.path(),
            include_bytes!("../../../examples/config/assets/sample.png"),
        )
        .unwrap();
        worker.join().unwrap();
        let (mut first, mut second) = UnixStream::pair().unwrap();
        let worker = std::thread::spawn(move || {
            first.write_all(b"sta").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
            first.write_all(b"tus\n").unwrap();
        });
        second
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let (command, fd) = receive_request(&mut second).unwrap();
        assert_eq!(command, "status\n");
        assert!(fd.is_none());
        worker.join().unwrap();
    }
}
