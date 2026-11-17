//! `enable_system_log` after something else installed the process's
//! global subscriber: its own test binary, for its own global state.

use aspero_ffi::{LogLevel, enable_system_log};

#[test]
fn the_system_log_leaves_another_subscriber_in_place() {
    tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default())
        .expect("the first global subscriber");
    assert!(!enable_system_log(
        "org.aspero.tests".into(),
        LogLevel::Info
    ));
}
