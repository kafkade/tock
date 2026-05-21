//! Cross-platform notifications.
//!
//! Phase 2 foundation: prints notification-style messages to stderr.
//! A follow-up PR can wire in `notify-rust` for desktop notifications
//! on Linux (D-Bus), macOS (`NSUserNotification`), and Windows (toast).

/// Send a notification.
pub fn notify(title: &str, body: &str) {
    eprintln!("🔔 {title}: {body}");
}
