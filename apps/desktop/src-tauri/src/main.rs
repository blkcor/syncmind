// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tracing_subscriber::fmt::init();

    #[cfg(target_os = "macos")]
    unsafe {
        // Override argv[0] so the process name shown in Activity Monitor
        // is "SyncMind" instead of the raw binary name.
        // In a bundled app CFBundleDisplayName takes precedence.
        extern "C" {
            fn _NSGetArgv() -> *mut *mut libc::c_char;
            fn _NSGetArgc() -> *mut libc::c_int;
        }
        let argc = *_NSGetArgc();
        let argv = _NSGetArgv();
        if argc > 0 && !argv.is_null() {
            let argv0 = *argv;
            let name = std::ffi::CString::new("SyncMind").unwrap();
            let old_len = libc::strlen(argv0);
            if name.as_bytes().len() <= old_len {
                libc::strcpy(argv0, name.as_ptr());
            }
        }
    }

    syncmind_desktop_lib::run();
}
