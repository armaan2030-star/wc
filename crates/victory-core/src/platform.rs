//! What kind of Windows the process is standing on.
//!
//! Running the Windows build on Linux under Proton/Wine is supported
//! (docs/proton.md). Wine's ntdll exports `wine_get_version`, which real
//! Windows does not, so we can name the host in start-up logs and
//! `victory config` output; a bug report that says which Wine it ran on is
//! worth a lot.

/// If the ntdll we are calling into is Wine's, the version string it reports
/// (the symbol exists since Wine 1.8); `None` on genuine Windows, and `None`
/// off Windows entirely.
#[cfg(windows)]
pub fn wine_version() -> Option<String> {
    use std::ffi::CStr;
    use std::mem::transmute;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

    let name: Vec<u16> = "ntdll.dll".encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: ntdll is always mapped in a Windows process; the string is
    // NUL-terminated UTF-16.
    let ntdll = unsafe { GetModuleHandleW(name.as_ptr()) };
    if ntdll.is_null() {
        return None;
    }
    // SAFETY: valid module handle; C-string literal is NUL-terminated.
    let addr = unsafe { GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast()) };
    let Some(addr) = addr else {
        return None; // genuine Windows: the symbol does not exist
    };
    // SAFETY: Wine exports this as `const char* wine_get_version(void)`; it
    // returns a static string owned by the process. Re-typing the resolved
    // entry point is how every dynamic import in this tree is bound.
    let get_version: unsafe extern "system" fn() -> *const i8 = unsafe { transmute(addr) };
    let text = unsafe { CStr::from_ptr(get_version()) };
    Some(text.to_string_lossy().into_owned())
}

/// Off Windows there is no Wine under our feet: we are the host.
#[cfg(not(windows))]
pub fn wine_version() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_host_names_itself_when_it_is_wine() {
        // Linux CI and the Windows CI both run on real hosts: `None`. The
        // `Some` side is covered by `proton/smoke.sh`, which greps the
        // banner this feeds (docs/proton.md).
        let seen = super::wine_version();
        if cfg!(windows) {
            if let Some(v) = seen {
                assert!(!v.is_empty(), "Wine reported an empty version");
            }
        } else {
            assert_eq!(seen, None);
        }
    }
}
