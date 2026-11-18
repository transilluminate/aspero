//! `enable_system_log` installs the process's global subscriber once: its
//! own test binary, so no other test's global state is touched.

use aspero_ffi::{LogLevel, enable_system_log};

#[test]
fn the_system_log_is_enabled_once() {
    // Never a subscriber: an empty subsystem, or one with a NUL.
    assert!(!enable_system_log(String::new(), LogLevel::Info));
    assert!(!enable_system_log("a\0b".into(), LogLevel::Info));
    let first = enable_system_log("org.aspero.tests".into(), LogLevel::Debug);
    assert_eq!(first, cfg!(target_vendor = "apple"));
    tracing::info!(target: "aspero::engine", op = "verify", outcome = "ok", "call");
    tracing::debug!(target: "aspero::net", provider = %"openalex", url = %"/works/x", "request_url");
    // Called again, it says no and leaves the first in place.
    assert!(!enable_system_log(
        "org.aspero.tests".into(),
        LogLevel::Info
    ));
}
