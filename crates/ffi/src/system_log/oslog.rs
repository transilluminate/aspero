//! The unified log, through a C shim (`oslog.c`, built by `build.rs`):
//! `os_log` is a macro, and its privacy is in the format string, which
//! must be a literal. The crate's only `unsafe` besides UniFFI's glue.

use std::collections::HashMap;
use std::ffi::{CString, c_char, c_void};
use std::ptr::NonNull;
use std::sync::Mutex;

use tracing::Level;

use super::Sink;

#[allow(unsafe_code)]
mod sys {
    use std::ffi::{c_char, c_void};

    unsafe extern "C" {
        pub fn aspero_os_log_create(
            subsystem: *const c_char,
            category: *const c_char,
        ) -> *mut c_void;
        pub fn aspero_os_log_enabled(log: *mut c_void, kind: u8) -> bool;
        pub fn aspero_os_log(
            log: *mut c_void,
            kind: u8,
            private_text: bool,
            message: *const c_char,
        );
    }
}

/// `os_log_type_t`'s values.
const DEFAULT: u8 = 0x00;
const DEBUG: u8 = 0x02;
const ERROR: u8 = 0x10;

/// An `os_log_t`, kept for the process's life.
struct Handle(NonNull<c_void>);

// SAFETY: os_log objects are immutable and safe to use from any thread
// (Apple's documentation of `os_log_create`).
#[allow(unsafe_code)]
unsafe impl Send for Handle {}
#[allow(unsafe_code)]
unsafe impl Sync for Handle {}

/// Writes to the unified log under one subsystem, a log object per
/// category made as first needed.
pub(super) struct OsLog {
    subsystem: CString,
    logs: Mutex<HashMap<String, Handle>>,
}

impl OsLog {
    /// `None` for a subsystem that can't be one: empty, or with a NUL.
    pub(super) fn new(subsystem: &str) -> Option<Self> {
        if subsystem.trim().is_empty() {
            return None;
        }
        Some(OsLog {
            subsystem: CString::new(subsystem).ok()?,
            logs: Mutex::default(),
        })
    }

    /// The category's log object (made the first time), the lock let go
    /// before it's used.
    fn log(&self, category: &str) -> Option<*mut c_void> {
        let mut logs = self
            .logs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(handle) = logs.get(category) {
            return Some(handle.0.as_ptr());
        }
        let name = CString::new(category).ok()?;
        #[allow(unsafe_code)]
        // SAFETY: both are valid C strings, alive for the call.
        let log = unsafe { sys::aspero_os_log_create(self.subsystem.as_ptr(), name.as_ptr()) };
        let handle = Handle(NonNull::new(log)?);
        let ptr = handle.0.as_ptr();
        logs.insert(category.to_owned(), handle);
        Some(ptr)
    }
}

/// Info and above as Apple's `Logger` levels them (warnings as errors,
/// info as notices: kept in the log store); debug and trace as debug.
fn kind(level: Level) -> u8 {
    match level {
        Level::ERROR | Level::WARN => ERROR,
        Level::INFO => DEFAULT,
        Level::DEBUG | Level::TRACE => DEBUG,
    }
}

impl Sink for OsLog {
    fn enabled(&self, category: &str, level: Level) -> bool {
        let Some(log) = self.log(category) else {
            return false;
        };
        #[allow(unsafe_code)]
        // SAFETY: a log object from `os_log_create`, never released.
        unsafe {
            sys::aspero_os_log_enabled(log, kind(level))
        }
    }

    fn write(&self, category: &str, level: Level, private: bool, line: &str) {
        let Ok(line) = CString::new(line.replace('\0', "")) else {
            return;
        };
        let message: *const c_char = line.as_ptr();
        let Some(log) = self.log(category) else {
            return;
        };
        #[allow(unsafe_code)]
        // SAFETY: a log object from `os_log_create`, never released, and a
        // valid C string alive for the call.
        unsafe {
            sys::aspero_os_log(log, kind(level), private, message);
        }
    }
}
