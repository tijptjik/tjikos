//! PAM module setting synthetic tokens, as only modules can set them.
#![cfg(target_os = "linux")]

use std::ffi::{CStr, c_char, c_int, c_void};

const PAM_SUCCESS: c_int = 0;
const PAM_AUTHTOK: c_int = 6;
const PAM_OLDAUTHTOK: c_int = 7;
const PAM_PRELIM_CHECK: c_int = 0x4000;

#[link(name = "libpam.so.0", modifiers = "+verbatim")]
unsafe extern "C" {
    fn pam_set_item(pamh: *mut c_void, item_type: c_int, item: *const c_void) -> c_int;
}

#[allow(clippy::missing_safety_doc)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pam_sm_authenticate(
    pamh: *mut c_void,
    _flags: c_int,
    argc: c_int,
    argv: *mut *const c_char,
) -> c_int {
    let mut reject = false;
    for i in 0..argc as usize {
        match unsafe { CStr::from_ptr(*argv.add(i)) }.to_bytes() {
            b"reject" => reject = true,
            b"no_token" => {
                return unsafe { pam_set_item(pamh, PAM_AUTHTOK, std::ptr::null()) };
            }
            b"empty_token" => {
                return unsafe { pam_set_item(pamh, PAM_AUTHTOK, c"".as_ptr().cast()) };
            }
            b"wrong_token" => {
                return unsafe {
                    pam_set_item(pamh, PAM_AUTHTOK, c"wrong-password".as_ptr().cast())
                };
            }
            _ => {}
        }
    }
    if reject {
        let ret = unsafe { pam_set_item(pamh, PAM_AUTHTOK, c"old-password".as_ptr().cast()) };
        return if ret == PAM_SUCCESS { 7 } else { ret };
    }
    unsafe { pam_set_item(pamh, PAM_AUTHTOK, c"old-password".as_ptr().cast()) }
}

#[allow(clippy::missing_safety_doc)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pam_sm_chauthtok(
    pamh: *mut c_void,
    flags: c_int,
    _argc: c_int,
    _argv: *mut *const c_char,
) -> c_int {
    if flags & PAM_PRELIM_CHECK != 0 {
        return PAM_SUCCESS;
    }
    let ret = unsafe { pam_set_item(pamh, PAM_OLDAUTHTOK, c"old-password".as_ptr().cast()) };
    if ret != PAM_SUCCESS {
        return ret;
    }
    unsafe { pam_set_item(pamh, PAM_AUTHTOK, c"new-password".as_ptr().cast()) }
}
