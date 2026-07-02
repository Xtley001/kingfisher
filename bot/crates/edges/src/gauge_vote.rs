//! Edge 6 — Gauge Vote Windows (Thursday)
//! Curve gauge weight votes happen weekly, ending Thursday ~00:00 UTC.
//! Liquidity shifts between pools in the hours surrounding the vote.
//! Calendar-predictable — pre-scan all pools Thursday morning.

/// Returns true if we're in the Thursday gauge vote window (Wed 20:00 - Thu 04:00 UTC).
pub fn is_gauge_vote_window() -> bool {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Thursday = day 4 in week (0=Mon, ..., 3=Thu with Unix epoch Mon=Thu)
    // Unix epoch Jan 1 1970 was a Thursday (day index 3 if Mon=0)
    let day_of_week = (secs / 86400 + 3) % 7; // 0=Mon, ..., 6=Sun
    let hour_of_day = (secs % 86400) / 3600;
    // Wednesday 20:00 - Thursday 04:00 UTC
    (day_of_week == 2 && hour_of_day >= 20) || (day_of_week == 3 && hour_of_day < 4)
}
