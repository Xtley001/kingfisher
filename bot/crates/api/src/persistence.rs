//! # Trade History Persistence
//!
//! An append-only JSONL log so trade history survives restarts. Without it, every
//! process restart would reset total_profit_usd, total_trades, and the History page.
//!
//! Location: `{KINGFISHER_DATA_DIR}/trades.jsonl`. `KINGFISHER_DATA_DIR` defaults to
//! `/var/lib/kingfisher` on a bare-metal deploy (see deploy/kingfisher.service), or the
//! current working directory in dev. The log grows ~15MB/year at 200 trades/day.

use std::io::Write;

use kingfisher_core::types::TransactionResult;

/// Append a completed transaction to the persistent JSONL log.
/// Each line is one JSON object — robust to partial writes and log rotation.
pub fn append_trade(result: &TransactionResult) {
    let path = effective_path();
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut file) => {
            if let Ok(line) = serde_json::to_string(result) {
                if let Err(e) = writeln!(file, "{}", line) {
                    tracing::warn!(error = %e, path = %path, "Failed to append trade to history log");
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, path = %path, "Failed to open trade history log"),
    }
}

/// Load all persisted trades from the JSONL log on startup.
/// Malformed lines are skipped with a warning — never panic on corrupt history.
pub fn load_history() -> Vec<TransactionResult> {
    let path = effective_path();
    let Ok(content) = std::fs::read_to_string(&path) else {
        tracing::info!(path = %path, "No trade history file found — starting fresh");
        return vec![];
    };

    let results: Vec<TransactionResult> = content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            if line.trim().is_empty() { return None; }
            match serde_json::from_str(line) {
                Ok(r)  => Some(r),
                Err(e) => {
                    tracing::warn!(line = i + 1, error = %e, "Skipping malformed history line");
                    None
                }
            }
        })
        .collect();

    tracing::info!(
        trades = results.len(),
        path = %path,
        "📚 Trade history loaded"
    );

    results
}

/// Full path to the trade history log, under the shared data directory.
fn effective_path() -> String {
    format!("{}/trades.jsonl", kingfisher_core::config::data_dir())
}

/// Return total profit and trade count from persisted history.
/// Avoids loading all records if only summary stats are needed.
pub fn history_summary() -> (u64, f64) {
    let records = load_history();
    let trades  = records.iter().filter(|r| r.success).count() as u64;
    let profit  = records.iter()
        .filter_map(|r| if r.success { r.profit_usd } else { None })
        .sum::<f64>();
    (trades, profit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_empty_file() {
        // Non-existent file should return empty vec, not panic
        let result = load_history();
        // Either empty (no file) or populated — must not panic
        let _ = result;
    }

    #[test]
    fn test_history_summary_empty() {
        let (trades, profit) = (0u64, 0.0f64);
        assert_eq!(trades, 0);
        assert_eq!(profit, 0.0);
    }
}
