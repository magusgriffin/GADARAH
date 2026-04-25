//! External alert dispatch — OS notifications + outbound webhooks.
//!
//! Every alert pushed into `SharedState.alerts` can optionally fan out to
//! two external channels:
//!   1. The desktop notification system (`notify-rust`). User sees a toast
//!      from the OS, even if GADARAH is minimised or behind another window.
//!   2. A webhook endpoint (Discord / Slack / generic JSON POST). Same
//!      payload across channels, formatted per `WebhookKind` so each
//!      receiver renders it natively.
//!
//! Both paths are best-effort: dispatch failures are logged at WARN and
//! never propagate up. The dispatch itself runs on a detached worker
//! thread so a slow webhook never blocks the UI lock.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::state::{Alert, AlertSeverity};

/// User-facing notification preferences. Persisted as JSON next to
/// `welcome_seen.flag` and `update_check.json` so the existing
/// `$CONFIG/gadarah/` directory stays the single home for app-level state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationSettings {
    /// Master switch for OS toasts.
    pub os_enabled: bool,
    /// Minimum severity that fires either channel. Anything below is
    /// silently dropped — saves the user from getting toasted on every
    /// Info-level update-check banner.
    pub min_severity: AlertSeverity,
    /// Webhook URL. Empty string disables the channel.
    pub webhook_url: String,
    /// Determines how the payload is formatted before POSTing.
    pub webhook_kind: WebhookKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebhookKind {
    Discord,
    Slack,
    /// Generic POST: body is `{"title": ..., "body": ..., "severity": ...}`
    /// with no platform-specific framing. Useful for self-hosted webhook
    /// receivers and IFTTT-style services.
    Generic,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            os_enabled: true,
            min_severity: AlertSeverity::Warning,
            webhook_url: String::new(),
            webhook_kind: WebhookKind::Discord,
        }
    }
}

impl NotificationSettings {
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("gadarah").join("notifications.json"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            tracing::warn!("notifications: config dir unavailable, settings will not persist");
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!(error = %e, "notifications: create_dir_all failed");
                return;
            }
        }
        match serde_json::to_vec_pretty(self) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&path, bytes) {
                    tracing::warn!(error = %e, path = %path.display(), "notifications: write failed");
                }
            }
            Err(e) => tracing::warn!(error = %e, "notifications: serialize failed"),
        }
    }

    fn passes_threshold(&self, severity: AlertSeverity) -> bool {
        severity_rank(severity) >= severity_rank(self.min_severity)
    }
}

fn severity_rank(s: AlertSeverity) -> u8 {
    match s {
        AlertSeverity::Info => 0,
        AlertSeverity::Warning => 1,
        AlertSeverity::Danger => 2,
    }
}

/// Fire OS notification + webhook for `alert`, off the calling thread.
/// Cheap to call (does the threshold check inline; only spawns work if
/// at least one channel will fire).
pub fn dispatch(alert: &Alert, settings: &NotificationSettings) {
    if !settings.passes_threshold(alert.severity) {
        return;
    }
    let want_os = settings.os_enabled;
    let want_webhook = !settings.webhook_url.trim().is_empty();
    if !want_os && !want_webhook {
        return;
    }

    let alert = alert.clone();
    let settings = settings.clone();
    std::thread::Builder::new()
        .name("gadarah-gui-notify".into())
        .spawn(move || {
            if want_os {
                if let Err(e) = send_os_notification(&alert) {
                    tracing::warn!(error = %e, title = %alert.title, "OS notify failed");
                }
            }
            if want_webhook {
                if let Err(e) = send_webhook(&alert, &settings) {
                    tracing::warn!(error = %e, title = %alert.title, "webhook failed");
                }
            }
        })
        .ok();
}

fn send_os_notification(alert: &Alert) -> Result<(), String> {
    let urgency = match alert.severity {
        AlertSeverity::Info => notify_rust::Urgency::Low,
        AlertSeverity::Warning => notify_rust::Urgency::Normal,
        AlertSeverity::Danger => notify_rust::Urgency::Critical,
    };
    let mut n = notify_rust::Notification::new();
    n.summary(&alert.title);
    if !alert.body.is_empty() {
        n.body(&alert.body);
    }
    n.appname("GADARAH");
    // `urgency` is libnotify-specific (Linux/BSD). On Windows/macOS the
    // call is silently ignored.
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        n.urgency(urgency);
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let _ = urgency;
    }
    n.show().map(|_| ()).map_err(|e| e.to_string())
}

fn send_webhook(alert: &Alert, settings: &NotificationSettings) -> Result<(), String> {
    let url = settings.webhook_url.trim();
    if url.is_empty() {
        return Ok(());
    }
    let body = match settings.webhook_kind {
        WebhookKind::Discord => discord_payload(alert),
        WebhookKind::Slack => slack_payload(alert),
        WebhookKind::Generic => generic_payload(alert),
    };
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("gadarah-gui/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("build client: {e}"))?;
    let resp = client
        .post(url)
        .json(&body)
        .send()
        .map_err(|e| format!("post: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "webhook returned {} ({})",
            resp.status(),
            resp.status().canonical_reason().unwrap_or("?")
        ));
    }
    Ok(())
}

fn severity_emoji(s: AlertSeverity) -> &'static str {
    match s {
        AlertSeverity::Info => "ℹ️",
        AlertSeverity::Warning => "⚠️",
        AlertSeverity::Danger => "🚨",
    }
}

fn severity_color(s: AlertSeverity) -> u32 {
    // Discord embed color values (decimal RGB).
    match s {
        AlertSeverity::Info => 0x3B82F6,    // blue
        AlertSeverity::Warning => 0xF59E0B, // amber
        AlertSeverity::Danger => 0xDC2626,  // red
    }
}

fn discord_payload(alert: &Alert) -> serde_json::Value {
    let emoji = severity_emoji(alert.severity);
    serde_json::json!({
        "username": "GADARAH",
        "embeds": [{
            "title": format!("{emoji} {}", alert.title),
            "description": if alert.body.is_empty() { "—".to_string() } else { alert.body.clone() },
            "color": severity_color(alert.severity),
            "timestamp": chrono::DateTime::from_timestamp(alert.timestamp, 0)
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        }],
    })
}

fn slack_payload(alert: &Alert) -> serde_json::Value {
    let emoji = severity_emoji(alert.severity);
    let header = format!("{emoji} *{}*", alert.title);
    let text = if alert.body.is_empty() {
        header
    } else {
        format!("{header}\n{}", alert.body)
    };
    serde_json::json!({
        "text": text,
    })
}

fn generic_payload(alert: &Alert) -> serde_json::Value {
    let severity = match alert.severity {
        AlertSeverity::Info => "info",
        AlertSeverity::Warning => "warning",
        AlertSeverity::Danger => "danger",
    };
    serde_json::json!({
        "title": alert.title,
        "body": alert.body,
        "severity": severity,
        "timestamp": alert.timestamp,
    })
}

/// Push a synthetic alert through the dispatcher so the user can sanity-check
/// their config from the UI. Fires the same OS-notify + webhook plumbing as
/// real alerts but uses a fixed title/body so receivers can tell it's a test.
pub fn send_test(settings: &NotificationSettings) {
    let alert = Alert {
        timestamp: chrono::Utc::now().timestamp(),
        severity: AlertSeverity::Info,
        title: "GADARAH test notification".to_string(),
        body: "If you see this, your notification settings are working.".to_string(),
        dismissed: false,
        action_url: None,
        action_update_wizard: false,
    };
    // Send-test deliberately ignores the severity threshold so users can
    // confirm both channels even when min_severity is set high.
    let bypass = NotificationSettings {
        min_severity: AlertSeverity::Info,
        ..settings.clone()
    };
    dispatch(&alert, &bypass);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_drops_lower_severity() {
        let mut s = NotificationSettings::default();
        s.min_severity = AlertSeverity::Warning;
        assert!(!s.passes_threshold(AlertSeverity::Info));
        assert!(s.passes_threshold(AlertSeverity::Warning));
        assert!(s.passes_threshold(AlertSeverity::Danger));
    }

    #[test]
    fn discord_payload_has_embed_color() {
        let alert = Alert {
            timestamp: 1000,
            severity: AlertSeverity::Danger,
            title: "kill switch".into(),
            body: "daily DD limit hit".into(),
            dismissed: false,
            action_url: None,
            action_update_wizard: false,
        };
        let v = discord_payload(&alert);
        assert_eq!(v["embeds"][0]["color"], 0xDC2626);
        assert!(v["embeds"][0]["title"]
            .as_str()
            .unwrap()
            .contains("kill switch"));
    }

    #[test]
    fn slack_payload_combines_title_and_body() {
        let alert = Alert {
            timestamp: 1000,
            severity: AlertSeverity::Warning,
            title: "vol halt".into(),
            body: "spread/atr ratio 0.42".into(),
            dismissed: false,
            action_url: None,
            action_update_wizard: false,
        };
        let v = slack_payload(&alert);
        let text = v["text"].as_str().unwrap();
        assert!(text.contains("vol halt"));
        assert!(text.contains("spread/atr"));
    }

    #[test]
    fn generic_payload_has_lowercase_severity() {
        let alert = Alert {
            timestamp: 1000,
            severity: AlertSeverity::Info,
            title: "t".into(),
            body: "b".into(),
            dismissed: false,
            action_url: None,
            action_update_wizard: false,
        };
        let v = generic_payload(&alert);
        assert_eq!(v["severity"], "info");
    }

    #[test]
    fn empty_webhook_url_skips_dispatch() {
        let mut s = NotificationSettings::default();
        s.webhook_url = "  ".into();
        s.os_enabled = false;
        // Just verifying the early-exit path doesn't panic; no thread spawned.
        let alert = Alert {
            timestamp: 0,
            severity: AlertSeverity::Danger,
            title: "x".into(),
            body: "y".into(),
            dismissed: false,
            action_url: None,
            action_update_wizard: false,
        };
        dispatch(&alert, &s);
    }
}
