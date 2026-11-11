//! Routes `tracing` output to logcat (Android drops stdout and stderr).

use std::ffi::CString;
use std::io::{self, Write};
use std::os::raw::{c_char, c_int};

const ANDROID_LOG_INFO: c_int = 4;

#[link(name = "log")]
#[allow(unsafe_code, reason = "liblog is the only way to write to logcat")]
unsafe extern "C" {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

/// A writer that sends each line to logcat under the tag `urushi`.
#[derive(Default)]
pub(crate) struct Logcat {
    line: Vec<u8>,
}

impl Logcat {
    fn flush_line(&mut self) {
        if self.line.is_empty() {
            return;
        }
        let text = CString::new(std::mem::take(&mut self.line)).unwrap_or_default();
        let tag = c"urushi";
        // SAFETY: both pointers are valid NUL-terminated strings that outlive the call.
        #[allow(unsafe_code, reason = "FFI call into liblog")]
        unsafe {
            __android_log_write(ANDROID_LOG_INFO, tag.as_ptr(), text.as_ptr());
        }
    }
}

impl Write for Logcat {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for &b in buf {
            if b == b'\n' {
                self.flush_line();
            } else {
                self.line.push(b);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_line();
        Ok(())
    }
}

impl Drop for Logcat {
    fn drop(&mut self) {
        self.flush_line();
    }
}

/// Installs the logcat subscriber, unless one is already installed in this process.
pub(crate) fn init() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            "info,urushi_app=debug,wgpu_core=warn,wgpu_hal=warn,naga=warn",
        ))
        .with_ansi(false)
        .with_writer(Logcat::default)
        .try_init();
}
