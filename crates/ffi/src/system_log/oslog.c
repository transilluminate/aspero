// The unified log for aspero's system log (`oslog.rs`): `os_log` is a
// macro, so Rust calls it through these. Each message is one string,
// formatted public or private.
#include <os/log.h>
#include <stdbool.h>
#include <stdint.h>

void *aspero_os_log_create(const char *subsystem, const char *category) {
    return (void *)os_log_create(subsystem, category);
}

bool aspero_os_log_enabled(void *log, uint8_t type) {
    return os_log_type_enabled((os_log_t)log, (os_log_type_t)type);
}

void aspero_os_log(void *log, uint8_t type, bool private_text, const char *message) {
    if (private_text) {
        os_log_with_type((os_log_t)log, (os_log_type_t)type, "%{private}s", message);
    } else {
        os_log_with_type((os_log_t)log, (os_log_type_t)type, "%{public}s", message);
    }
}
