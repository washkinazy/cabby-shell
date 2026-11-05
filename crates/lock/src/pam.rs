//! Linux-PAM ABI. PAM runs only in the short-lived authentication helper.
use crate::{Secret, auth::Error};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    os::unix::fs::MetadataExt,
    ptr,
};
#[repr(C)]
struct Message {
    style: c_int,
    message: *const c_char,
}
#[repr(C)]
struct Response {
    response: *mut c_char,
    code: c_int,
}
#[repr(C)]
struct Conversation {
    function: unsafe extern "C" fn(
        c_int,
        *const *const Message,
        *mut *mut Response,
        *mut c_void,
    ) -> c_int,
    data: *mut c_void,
}
// Linux-PAM's stable runtime ABI; no optional modules or authentication policy are bundled.
#[link(name = "libpam.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conversation: *const Conversation,
        handle: *mut *mut c_void,
    ) -> c_int;
    #[cfg(test)]
    fn pam_start_confdir(
        service: *const c_char,
        user: *const c_char,
        conversation: *const Conversation,
        directory: *const c_char,
        handle: *mut *mut c_void,
    ) -> c_int;
    fn pam_authenticate(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_acct_mgmt(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_get_item(handle: *const c_void, item: c_int, value: *mut *const c_void) -> c_int;
    fn pam_end(handle: *mut c_void, status: c_int) -> c_int;
}
struct Context<'a> {
    secret: &'a Secret,
    user: &'a CStr,
    supplied: bool,
}
unsafe extern "C" fn converse(
    count: c_int,
    messages: *const *const Message,
    replies: *mut *mut Response,
    data: *mut c_void,
) -> c_int {
    if !(1..=16).contains(&count) || messages.is_null() || replies.is_null() || data.is_null() {
        return 19;
    }
    // SAFETY: Linux-PAM provides count message pointers and the registered live Context.
    unsafe {
        *replies = ptr::null_mut();
        let context = &mut *data.cast::<Context<'_>>();
        let responses =
            libc::calloc(count as usize, std::mem::size_of::<Response>()).cast::<Response>();
        if responses.is_null() {
            return 5;
        }
        for index in 0..count as usize {
            let message = *messages.add(index);
            let bytes = if message.is_null() {
                None
            } else {
                match (*message).style {
                    1 if !context.supplied => {
                        context.supplied = true;
                        Some(context.secret.as_bytes())
                    }
                    2 => Some(context.user.to_bytes()),
                    3 | 4 => Some(&[][..]), // Never log/display module text that could contain sensitive data.
                    _ => None,
                }
            };
            let Some(bytes) = bytes else {
                free_responses(responses, count as usize);
                return 19;
            };
            if !bytes.is_empty() {
                let value = libc::calloc(bytes.len() + 1, 1).cast::<u8>();
                if value.is_null() {
                    free_responses(responses, count as usize);
                    return 5;
                }
                ptr::copy_nonoverlapping(bytes.as_ptr(), value, bytes.len());
                (*responses.add(index)).response = value.cast();
            }
        }
        *replies = responses; // Ownership transfers to PAM, which frees its conversation replies.
    }
    0
}
unsafe fn free_responses(responses: *mut Response, count: usize) {
    // SAFETY: only our calloc-allocated, NUL-terminated responses are passed here.
    unsafe {
        for index in 0..count {
            let response = (*responses.add(index)).response;
            if !response.is_null() {
                libc::explicit_bzero(response.cast(), libc::strlen(response));
                libc::free(response.cast());
            }
        }
        libc::free(responses.cast());
    }
}
pub fn current_user() -> Result<CString, Error> {
    let mut storage = vec![0u8; 65536];
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = ptr::null_mut();
    // SAFETY: buffers have the advertised sizes and getpwuid_r initializes entry on success.
    unsafe {
        if libc::getuid() != libc::geteuid()
            || libc::getpwuid_r(
                libc::getuid(),
                entry.as_mut_ptr(),
                storage.as_mut_ptr().cast(),
                storage.len(),
                &mut result,
            ) != 0
            || result.is_null()
        {
            return Err(Error::Unavailable);
        }
        let entry = entry.assume_init();
        if entry.pw_name.is_null() {
            return Err(Error::Unavailable);
        }
        Ok(CStr::from_ptr(entry.pw_name).to_owned())
    }
}
pub fn validate_policy() -> Result<(), Error> {
    let metadata = std::fs::metadata("/etc/pam.d/cabby-lock").map_err(|_| Error::Unavailable)?;
    if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
        return Err(Error::Unavailable);
    }
    Ok(())
}
pub fn authenticate(user: &CStr, secret: &Secret, directory: Option<&CStr>) -> Result<(), Error> {
    if secret.is_empty() {
        return Err(Error::Denied);
    }
    let mut context = Context {
        secret,
        user,
        supplied: false,
    };
    let conversation = Conversation {
        function: converse,
        data: (&mut context as *mut Context<'_>).cast(),
    };
    let mut handle = ptr::null_mut();
    // SAFETY: C strings/context live through pam_end; PAM owns and validates its opaque handle.
    unsafe {
        #[cfg(test)]
        let start = if let Some(directory) = directory {
            pam_start_confdir(
                c"cabby-lock".as_ptr(),
                user.as_ptr(),
                &conversation,
                directory.as_ptr(),
                &mut handle,
            )
        } else {
            pam_start(
                c"cabby-lock".as_ptr(),
                user.as_ptr(),
                &conversation,
                &mut handle,
            )
        };
        #[cfg(not(test))]
        let start = {
            let _ = directory;
            pam_start(
                c"cabby-lock".as_ptr(),
                user.as_ptr(),
                &conversation,
                &mut handle,
            )
        };
        if start != 0 {
            return Err(Error::Unavailable);
        }
        let mut status = pam_authenticate(handle, 1); // PAM_DISALLOW_NULL_AUTHTOK.
        if status == 0 {
            status = pam_acct_mgmt(handle, 1);
        }
        if status == 0 {
            let mut actual = ptr::null();
            if pam_get_item(handle, 2, &mut actual) != 0
                || actual.is_null()
                || CStr::from_ptr(actual.cast()) != user
            {
                status = 7;
            }
        }
        let end = pam_end(handle, status);
        if status == 0 && end == 0 {
            Ok(())
        } else {
            Err(Error::Denied)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_pam_stacks_accept_deny_and_reject_expired_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let path = CString::new(dir.path().as_os_str().as_encoded_bytes()).unwrap();
        let mut secret = Secret::default();
        secret.append("synthetic-not-a-user-password");
        for (auth, account, success) in [
            ("pam_permit.so", "pam_permit.so", true),
            ("pam_deny.so", "pam_permit.so", false),
            ("pam_permit.so", "pam_deny.so", false),
            ("cabby_missing.so", "pam_permit.so", false),
        ] {
            std::fs::write(
                dir.path().join("cabby-lock"),
                format!("auth required {auth}\naccount required {account}\n"),
            )
            .unwrap();
            assert_eq!(
                authenticate(c"fixture-user", &secret, Some(&path)).is_ok(),
                success
            );
        }
    }
    #[test]
    fn conversation_rejects_multiple_password_prompts_and_cleans_partial_replies() {
        let mut secret = Secret::default();
        secret.append("fixture");
        let mut context = Context {
            secret: &secret,
            user: c"fixture",
            supplied: false,
        };
        let message = Message {
            style: 1,
            message: c"Password".as_ptr(),
        };
        let messages = [&message as *const Message];
        let mut replies = ptr::null_mut();
        // SAFETY: provide the same pointer/ownership contract as Linux-PAM.
        unsafe {
            assert_eq!(
                converse(
                    1,
                    messages.as_ptr(),
                    &mut replies,
                    (&mut context as *mut Context<'_>).cast()
                ),
                0
            );
            assert_eq!(CStr::from_ptr((*replies).response).to_bytes(), b"fixture");
            free_responses(replies, 1);
            assert_eq!(
                converse(
                    1,
                    messages.as_ptr(),
                    &mut replies,
                    (&mut context as *mut Context<'_>).cast()
                ),
                19
            );
            assert!(replies.is_null());
        }
    }
}
