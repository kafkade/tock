//! Subscription tiers, account model, and usage tracking.
//!
//! The hosted service runs the same `tock-server` binary with
//! `--mode hosted`. It adds user accounts, subscription tiers, and
//! usage tracking. **Usage tracks only encrypted byte counts and event
//! counts — never content.**

use serde::{Deserialize, Serialize};

/// Server operating mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMode {
    /// Self-hosted: no accounts, no billing, no quotas.
    SelfHosted,
    /// Hosted service: accounts, subscriptions, quotas, rate limits.
    Hosted,
}

impl ServerMode {
    /// Parse from a string (`"self-hosted"` or `"hosted"`).
    ///
    /// # Errors
    /// Returns `None` for unrecognized values.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "self-hosted" | "selfhosted" => Some(Self::SelfHosted),
            "hosted" => Some(Self::Hosted),
            _ => None,
        }
    }
}

impl std::fmt::Display for ServerMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelfHosted => f.write_str("self-hosted"),
            Self::Hosted => f.write_str("hosted"),
        }
    }
}

/// Subscription tiers per architecture §11.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    /// Single device or local-network sync only.
    Free,
    /// Unlimited devices, hosted relay, encrypted backups.
    Personal,
    /// Up to 6 vaults under one bill.
    Family,
    /// Priority support, longer backup retention.
    Pro,
    /// Self-hosted: no limits enforced by the server.
    SelfHosted,
}

#[allow(dead_code)]
impl Tier {
    /// Maximum encrypted bytes stored for this tier.
    #[must_use]
    pub const fn max_storage_bytes(self) -> u64 {
        match self {
            Self::Free => 50 * 1024 * 1024,         // 50 MiB
            Self::Personal => 500 * 1024 * 1024,    // 500 MiB
            Self::Family => 2 * 1024 * 1024 * 1024, // 2 GiB
            Self::Pro => 10 * 1024 * 1024 * 1024,   // 10 GiB
            Self::SelfHosted => u64::MAX,
        }
    }

    /// Maximum events per push batch.
    #[must_use]
    pub const fn max_events_per_push(self) -> usize {
        match self {
            Self::Free => 64,
            Self::Personal | Self::Family | Self::Pro | Self::SelfHosted => 256,
        }
    }

    /// Maximum devices for this tier.
    #[must_use]
    pub const fn max_devices(self) -> usize {
        match self {
            Self::Free => 1,
            Self::Personal => 10,
            Self::Family => 60, // 6 vaults × 10 devices
            Self::Pro => 50,
            Self::SelfHosted => usize::MAX,
        }
    }

    /// Requests per minute rate limit.
    #[must_use]
    pub const fn requests_per_minute(self) -> u32 {
        match self {
            Self::Free => 30,
            Self::Personal | Self::Family => 120,
            Self::Pro => 300,
            Self::SelfHosted => u32::MAX,
        }
    }

    /// Canonical name for display / serialization.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Personal => "personal",
            Self::Family => "family",
            Self::Pro => "pro",
            Self::SelfHosted => "self-hosted",
        }
    }

    /// Parse from string.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "free" => Some(Self::Free),
            "personal" => Some(Self::Personal),
            "family" => Some(Self::Family),
            "pro" => Some(Self::Pro),
            "self-hosted" | "selfhosted" => Some(Self::SelfHosted),
            _ => None,
        }
    }
}

/// Usage snapshot for an account (encrypted byte counts only, never
/// content).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageSnapshot {
    /// Total encrypted bytes stored across all vaults.
    pub bytes_stored: u64,
    /// Total event count across all vaults.
    pub event_count: u64,
    /// Number of registered devices.
    pub device_count: u64,
    /// Number of vaults.
    pub vault_count: u64,
}
