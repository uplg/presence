//! Record of the weeks whose report was actually emailed, keyed by Monday.
//! Prevents double sends and lets the daemon notice a missed Friday.

use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Default)]
struct LedgerFile {
    sent: Vec<NaiveDate>,
}

/// `~/.presence/sent.json` (override with `PRESENCE_LEDGER_FILE` for tests).
fn default_path() -> PathBuf {
    std::env::var_os("PRESENCE_LEDGER_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::data_dir().join("sent.json"))
}

/// Mondays of every week already sent, sorted.
pub fn load() -> Result<Vec<NaiveDate>> {
    load_from(&default_path())
}

pub fn mark_sent(monday: NaiveDate) -> Result<()> {
    mark_sent_in(&default_path(), monday)
}

fn load_from(path: &Path) -> Result<Vec<NaiveDate>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let file: LedgerFile = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(file.sent)
}

fn mark_sent_in(path: &Path, monday: NaiveDate) -> Result<()> {
    let mut sent = load_from(path)?;
    if sent.contains(&monday) {
        return Ok(());
    }
    sent.push(monday);
    sent.sort();
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(&LedgerFile { sent })?;
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "presence_ledger_test_{}_{}.json",
            std::process::id(),
            n
        ))
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn missing_file_is_empty() {
        assert!(load_from(&temp_path()).unwrap().is_empty());
    }

    #[test]
    fn mark_sent_is_sorted_and_idempotent() {
        let p = temp_path();
        mark_sent_in(&p, d(2026, 9, 14)).unwrap();
        mark_sent_in(&p, d(2026, 9, 7)).unwrap();
        mark_sent_in(&p, d(2026, 9, 14)).unwrap();
        assert_eq!(load_from(&p).unwrap(), vec![d(2026, 9, 7), d(2026, 9, 14)]);
        std::fs::remove_file(&p).ok();
    }
}
