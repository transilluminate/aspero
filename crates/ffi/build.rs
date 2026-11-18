//! Builds the `os_log` shim (`src/system_log/oslog.c`) on Apple targets.

fn main() {
    println!("cargo:rerun-if-changed=src/system_log/oslog.c");
    if std::env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
        cc::Build::new()
            .file("src/system_log/oslog.c")
            .compile("aspero_oslog");
    }
}
