//! Cross-platform notifications.
//!
//! Phase 2 foundation: prints notification-style messages to stderr.
//! A follow-up PR can wire in `notify-rust` for desktop notifications
//! on Linux (D-Bus), macOS (`NSUserNotification`), and Windows (toast).

use std::sync::atomic::{AtomicBool, Ordering};

/// Master switch for notifications, configured from `[notifications] enabled`.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Enable or disable notifications globally (called once from config at
/// startup).
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Send a notification, unless notifications are disabled in config.
pub fn notify(title: &str, body: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    eprintln!("🔔 {title}: {body}");
}
