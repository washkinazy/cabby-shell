//! Lock policy and bounded, out-of-process authentication. No Wayland or widgets.
pub mod auth;
mod pam;
pub mod policy;
use std::io::Read;
pub const MAX_SECRET: usize = 1024;
/// Fixed-capacity credential storage; debug output never contains its value.
pub struct Secret {
    bytes: [u8; MAX_SECRET],
    len: usize,
}
impl Default for Secret {
    fn default() -> Self {
        Self {
            bytes: [0; MAX_SECRET],
            len: 0,
        }
    }
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([redacted])")
    }
}
impl Secret {
    pub fn append(&mut self, text: &str) -> bool {
        if text.contains('\0')
            || text.chars().any(char::is_control)
            || self.len + text.len() > MAX_SECRET
        {
            return false;
        }
        self.bytes[self.len..self.len + text.len()].copy_from_slice(text.as_bytes());
        self.len += text.len();
        true
    }
    pub fn backspace(&mut self) {
        let Some((offset, _)) = self.as_str().char_indices().next_back() else {
            return;
        };
        wipe(&mut self.bytes[offset..self.len]);
        self.len = offset;
    }
    pub fn clear(&mut self) {
        wipe(&mut self.bytes);
        self.len = 0;
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
    fn as_str(&self) -> &str {
        std::str::from_utf8(self.as_bytes()).expect("validated secret UTF-8")
    }
    pub fn masked(&self) -> String {
        "•".repeat(self.as_str().chars().count().min(32))
    }
    pub fn read(reader: &mut impl Read) -> std::io::Result<Self> {
        let mut length = [0u8; 4];
        reader.read_exact(&mut length)?;
        let len = u32::from_be_bytes(length) as usize;
        if len == 0 || len > MAX_SECRET {
            return Err(std::io::Error::other("invalid credential frame"));
        }
        let mut secret = Self::default();
        reader.read_exact(&mut secret.bytes[..len])?;
        secret.len = len;
        if secret.as_bytes().contains(&0) || std::str::from_utf8(secret.as_bytes()).is_err() {
            return Err(std::io::Error::other("invalid credential frame"));
        }
        Ok(secret)
    }
}
impl Drop for Secret {
    fn drop(&mut self) {
        self.clear();
    }
}
fn wipe(bytes: &mut [u8]) {
    // SAFETY: the slice is writable for its exact length. explicit_bzero cannot be optimized away.
    unsafe {
        libc::explicit_bzero(bytes.as_mut_ptr().cast(), bytes.len());
    }
}
pub fn disable_dumps() -> std::io::Result<()> {
    // SAFETY: Linux prctl arguments disable process core dumps/ptrace access.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
/// The helper only authenticates the real UID through the fixed system policy.
pub fn authenticate_current(secret: &Secret) -> Result<(), auth::Error> {
    let user = pam::current_user()?;
    pam::validate_policy()?;
    pam::authenticate(&user, secret, None)
}
pub fn preflight() -> Result<(), auth::Error> {
    pam::current_user()?;
    pam::validate_policy()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_bound_utf8_editing_frames_and_debug() {
        let mut secret = Secret::default();
        assert!(secret.append("passé"));
        assert_eq!(secret.masked(), "•••••");
        secret.backspace();
        assert_eq!(secret.as_bytes(), b"pass");
        assert!(!secret.append("\0"));
        assert!(!secret.append(&"x".repeat(MAX_SECRET)));
        assert!(!format!("{secret:?}").contains("pass"));
        let mut wire = 4u32.to_be_bytes().to_vec();
        wire.extend(b"test");
        assert_eq!(
            Secret::read(&mut wire.as_slice()).unwrap().as_bytes(),
            b"test"
        );
        assert!(Secret::read(&mut 0u32.to_be_bytes().as_slice()).is_err());
        secret.clear();
        assert!(secret.bytes.iter().all(|b| *b == 0));
        assert!(secret.is_empty());
    }
}
