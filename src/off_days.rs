use anyhow::{Context, Result, bail};
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A range of off days (inclusive on both ends). A single day is start == end.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct OffEntry {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl OffEntry {
    pub fn range(start: NaiveDate, end: NaiveDate) -> Result<Self> {
        if end < start {
            bail!("end date {end} is before start date {start}");
        }
        Ok(Self { start, end })
    }

    /// Expand the inclusive range into individual dates.
    pub fn dates(&self) -> Vec<NaiveDate> {
        let mut out = Vec::new();
        let mut d = self.start;
        while d <= self.end {
            out.push(d);
            d += Duration::days(1);
        }
        out
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct OffDaysFile {
    entries: Vec<OffEntry>,
}

/// Resolve the on-disk location. Daemon and CLI must agree regardless of CWD,
/// so we anchor on `$HOME` (override with `PRESENCE_OFF_DAYS_FILE` for tests).
fn default_path() -> PathBuf {
    if let Ok(p) = std::env::var("PRESENCE_OFF_DAYS_FILE") {
        return PathBuf::from(p);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".presence").join("off_days.json");
    }
    PathBuf::from("off_days.json")
}

pub fn load() -> Result<Vec<OffEntry>> {
    load_from(&default_path())
}

/// All off days, expanded from ranges. Used by the report pipeline.
pub fn load_expanded() -> Result<Vec<NaiveDate>> {
    Ok(load()?.iter().flat_map(|e| e.dates()).collect())
}

pub fn add(entry: OffEntry) -> Result<bool> {
    add_in(&default_path(), entry)
}

/// Remove the entry whose start date matches `date`. Returns true if removed.
pub fn remove_by_start(date: NaiveDate) -> Result<bool> {
    remove_in(&default_path(), date)
}

fn load_from(path: &Path) -> Result<Vec<OffEntry>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let file: OffDaysFile = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    for e in &file.entries {
        if e.end < e.start {
            bail!(
                "invalid entry in {}: end {} before start {}",
                path.display(),
                e.end,
                e.start
            );
        }
    }
    Ok(file.entries)
}

fn save_to(path: &Path, entries: &[OffEntry]) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|e| (e.start, e.end));
    sorted.dedup();
    let file = OffDaysFile { entries: sorted };
    let content = serde_json::to_string_pretty(&file)?;
    std::fs::write(path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn add_in(path: &Path, entry: OffEntry) -> Result<bool> {
    let mut entries = load_from(path)?;
    if entries.contains(&entry) {
        return Ok(false);
    }
    entries.push(entry);
    save_to(path, &entries)?;
    Ok(true)
}

fn remove_in(path: &Path, date: NaiveDate) -> Result<bool> {
    let mut entries = load_from(path)?;
    let before = entries.len();
    entries.retain(|e| e.start != date);
    let removed = entries.len() < before;
    if removed {
        save_to(path, &entries)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "presence_off_days_test_{}_{}.json",
            std::process::id(),
            n
        ))
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn single(date: NaiveDate) -> OffEntry {
        OffEntry::range(date, date).unwrap()
    }

    #[test]
    fn single_entry_expands_to_one_date() {
        let e = single(d(2026, 5, 15));
        assert_eq!(e.dates(), vec![d(2026, 5, 15)]);
    }

    #[test]
    fn range_rejects_end_before_start() {
        assert!(OffEntry::range(d(2026, 7, 15), d(2026, 7, 1)).is_err());
    }

    #[test]
    fn range_dates_expand_inclusive() {
        let e = OffEntry::range(d(2026, 7, 1), d(2026, 7, 3)).unwrap();
        assert_eq!(e.dates(), vec![d(2026, 7, 1), d(2026, 7, 2), d(2026, 7, 3)]);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let p = temp_path();
        assert!(!p.exists());
        assert_eq!(load_from(&p).unwrap(), Vec::<OffEntry>::new());
    }

    #[test]
    fn add_then_load_roundtrip() {
        let p = temp_path();
        let e = single(d(2026, 5, 15));
        assert!(add_in(&p, e.clone()).unwrap());
        assert_eq!(load_from(&p).unwrap(), vec![e]);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn add_range_then_load_roundtrip() {
        let p = temp_path();
        let e = OffEntry::range(d(2026, 7, 1), d(2026, 7, 15)).unwrap();
        assert!(add_in(&p, e.clone()).unwrap());
        assert_eq!(load_from(&p).unwrap(), vec![e]);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn add_duplicate_returns_false() {
        let p = temp_path();
        let e = single(d(2026, 5, 15));
        assert!(add_in(&p, e.clone()).unwrap());
        assert!(!add_in(&p, e).unwrap());
        assert_eq!(load_from(&p).unwrap().len(), 1);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn add_keeps_entries_sorted_by_start() {
        let p = temp_path();
        add_in(&p, single(d(2026, 7, 1))).unwrap();
        add_in(&p, single(d(2026, 5, 15))).unwrap();
        add_in(&p, OffEntry::range(d(2026, 6, 1), d(2026, 6, 10)).unwrap()).unwrap();
        let entries = load_from(&p).unwrap();
        assert_eq!(entries[0].start, d(2026, 5, 15));
        assert_eq!(entries[1].start, d(2026, 6, 1));
        assert_eq!(entries[2].start, d(2026, 7, 1));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn remove_by_start_existing_returns_true() {
        let p = temp_path();
        add_in(&p, OffEntry::range(d(2026, 7, 1), d(2026, 7, 15)).unwrap()).unwrap();
        assert!(remove_in(&p, d(2026, 7, 1)).unwrap());
        assert!(load_from(&p).unwrap().is_empty());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn remove_by_start_missing_returns_false() {
        let p = temp_path();
        add_in(&p, single(d(2026, 5, 15))).unwrap();
        assert!(!remove_in(&p, d(2026, 6, 1)).unwrap());
        assert_eq!(load_from(&p).unwrap().len(), 1);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn load_malformed_file_errors() {
        let p = temp_path();
        std::fs::write(&p, "not json").unwrap();
        assert!(load_from(&p).is_err());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn load_invalid_range_errors() {
        let p = temp_path();
        std::fs::write(
            &p,
            r#"{"entries":[{"start":"2026-07-15","end":"2026-07-01"}]}"#,
        )
        .unwrap();
        assert!(load_from(&p).is_err());
        std::fs::remove_file(&p).ok();
    }
}
