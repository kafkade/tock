//! Idle detection for active time tracking (architecture §2.3.5, Phase 2.5).
//!
//! tock's timer is not a long-lived daemon, so "in-terminal-only" idle
//! detection works from a *last-activity heartbeat*: while a timer or focus
//! session runs, each CLI invocation records the current time. When the user
//! next stops (or inspects) the timer, the trailing gap between that heartbeat
//! and now is evaluated here. This module is pure — it computes whether an idle
//! interval occurred and how to resolve it, leaving persistence and prompting
//! to the platform layer.

use time::{Duration, OffsetDateTime};

/// How the idle interval should be applied when a timer is stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleResolution {
    /// Count the idle time as worked; the block ends at `now`, unchanged.
    Keep,
    /// Drop the idle time; the block is truncated to end at the last activity.
    Discard,
    /// Truncate the block at the last activity and record the idle interval as
    /// a separate block, so the time is preserved but not counted as focus.
    Split,
}

impl IdleResolution {
    /// Canonical lower-case string representation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Discard => "discard",
            Self::Split => "split",
        }
    }

    /// Parse from a case-insensitive string (`keep`/`k`, `discard`/`d`,
    /// `split`/`s`).
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "keep" | "k" => Some(Self::Keep),
            "discard" | "d" => Some(Self::Discard),
            "split" | "s" => Some(Self::Split),
            _ => None,
        }
    }
}

/// A detected idle interval: the span between the last recorded activity and
/// the moment the timer was stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleInterval {
    /// When activity was last observed (start of the idle span).
    pub start: OffsetDateTime,
    /// When the timer was stopped (end of the idle span).
    pub end: OffsetDateTime,
}

impl IdleInterval {
    /// Length of the idle interval.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.end - self.start
    }
}

/// Detect a trailing idle interval.
///
/// Returns `Some` only when `now - last_activity` is at least `threshold`.
/// Non-positive thresholds and clocks that appear to move backwards (an idle
/// gap that is not strictly positive) yield `None`, so callers never resolve a
/// zero-length or negative interval.
#[must_use]
pub fn detect(
    last_activity: OffsetDateTime,
    now: OffsetDateTime,
    threshold: Duration,
) -> Option<IdleInterval> {
    if threshold <= Duration::ZERO {
        return None;
    }
    let gap = now - last_activity;
    if gap <= Duration::ZERO || gap < threshold {
        return None;
    }
    Some(IdleInterval {
        start: last_activity,
        end: now,
    })
}

/// The result of resolving an idle interval against a running block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleOutcome {
    /// Timestamp the (previously running) block should end at.
    pub block_end: OffsetDateTime,
    /// A separate idle block to record, if the resolution splits it out.
    pub spillover: Option<IdleInterval>,
}

/// Resolve an idle interval into a concrete outcome for the running block.
///
/// `now` is the moment the timer was stopped (the natural end of the block when
/// the idle time is kept).
#[must_use]
pub const fn resolve(
    now: OffsetDateTime,
    idle: IdleInterval,
    resolution: IdleResolution,
) -> IdleOutcome {
    match resolution {
        IdleResolution::Keep => IdleOutcome {
            block_end: now,
            spillover: None,
        },
        IdleResolution::Discard => IdleOutcome {
            block_end: idle.start,
            spillover: None,
        },
        IdleResolution::Split => IdleOutcome {
            block_end: idle.start,
            spillover: Some(idle),
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{IdleResolution, detect, resolve};
    use time::{Duration, OffsetDateTime};

    fn epoch(secs: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000 + secs).expect("valid timestamp")
    }

    #[test]
    fn resolution_roundtrip() {
        for res in [
            IdleResolution::Keep,
            IdleResolution::Discard,
            IdleResolution::Split,
        ] {
            assert_eq!(IdleResolution::from_str_opt(res.as_str()), Some(res));
        }
        assert_eq!(
            IdleResolution::from_str_opt("K"),
            Some(IdleResolution::Keep)
        );
        assert_eq!(
            IdleResolution::from_str_opt(" Discard "),
            Some(IdleResolution::Discard)
        );
        assert_eq!(IdleResolution::from_str_opt("nope"), None);
    }

    #[test]
    fn detect_below_threshold_is_none() {
        let start = epoch(0);
        let now = epoch(299);
        assert!(detect(start, now, Duration::minutes(5)).is_none());
    }

    #[test]
    fn detect_at_threshold_is_some() {
        let start = epoch(0);
        let now = epoch(300);
        let idle = detect(start, now, Duration::minutes(5)).expect("idle detected");
        assert_eq!(idle.start, start);
        assert_eq!(idle.end, now);
        assert_eq!(idle.duration(), Duration::minutes(5));
    }

    #[test]
    fn detect_above_threshold_is_some() {
        let idle = detect(epoch(0), epoch(3600), Duration::minutes(5)).expect("idle detected");
        assert_eq!(idle.duration(), Duration::hours(1));
    }

    #[test]
    fn detect_backwards_clock_is_none() {
        assert!(detect(epoch(100), epoch(50), Duration::minutes(5)).is_none());
    }

    #[test]
    fn detect_zero_threshold_is_none() {
        assert!(detect(epoch(0), epoch(300), Duration::ZERO).is_none());
    }

    #[test]
    fn resolve_keep_leaves_block_unchanged() {
        let idle = detect(epoch(0), epoch(600), Duration::minutes(5)).unwrap();
        let outcome = resolve(epoch(600), idle, IdleResolution::Keep);
        assert_eq!(outcome.block_end, epoch(600));
        assert!(outcome.spillover.is_none());
    }

    #[test]
    fn resolve_discard_truncates_at_last_activity() {
        let idle = detect(epoch(0), epoch(600), Duration::minutes(5)).unwrap();
        let outcome = resolve(epoch(600), idle, IdleResolution::Discard);
        assert_eq!(outcome.block_end, epoch(0));
        assert!(outcome.spillover.is_none());
    }

    #[test]
    fn resolve_split_truncates_and_records_spillover() {
        let idle = detect(epoch(0), epoch(600), Duration::minutes(5)).unwrap();
        let outcome = resolve(epoch(600), idle, IdleResolution::Split);
        assert_eq!(outcome.block_end, epoch(0));
        let spill = outcome.spillover.expect("spillover recorded");
        assert_eq!(spill.start, epoch(0));
        assert_eq!(spill.end, epoch(600));
    }
}
