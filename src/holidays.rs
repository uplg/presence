use chrono::{Datelike, NaiveDate};

/// Compute Easter Sunday for a given year using the Anonymous Gregorian algorithm.
fn easter(year: i32) -> NaiveDate {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    NaiveDate::from_ymd_opt(year, month as u32, day as u32).unwrap()
}

/// Returns all French public holidays for the given year.
pub fn french_holidays(year: i32) -> Vec<(NaiveDate, &'static str)> {
    let easter_sunday = easter(year);
    let d = |m: u32, d: u32| NaiveDate::from_ymd_opt(year, m, d).unwrap();

    vec![
        (d(1, 1), "Jour de l'An"),
        (easter_sunday + chrono::Duration::days(1), "Lundi de Pâques"),
        (d(5, 1), "Fête du Travail"),
        (d(5, 8), "Victoire 1945"),
        (easter_sunday + chrono::Duration::days(39), "Ascension"),
        (
            easter_sunday + chrono::Duration::days(50),
            "Lundi de Pentecôte",
        ),
        (d(7, 14), "Fête Nationale"),
        (d(8, 15), "Assomption"),
        (d(11, 1), "Toussaint"),
        (d(11, 11), "Armistice"),
        (d(12, 25), "Noël"),
    ]
}

/// Check if a date is a French public holiday; returns the name if so.
pub fn is_holiday(date: NaiveDate) -> Option<&'static str> {
    french_holidays(date.year())
        .into_iter()
        .find(|(d, _)| *d == date)
        .map(|(_, name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    // --- Easter algorithm ---

    #[test]
    fn easter_2024() {
        assert_eq!(easter(2024), d(2024, 3, 31));
    }

    #[test]
    fn easter_2025() {
        assert_eq!(easter(2025), d(2025, 4, 20));
    }

    #[test]
    fn easter_2026() {
        assert_eq!(easter(2026), d(2026, 4, 5));
    }

    #[test]
    fn easter_2027() {
        assert_eq!(easter(2027), d(2027, 3, 28));
    }

    #[test]
    fn easter_2030() {
        assert_eq!(easter(2030), d(2030, 4, 21));
    }

    // --- Holiday count ---

    #[test]
    fn always_11_holidays() {
        for year in 2024..=2035 {
            assert_eq!(french_holidays(year).len(), 11, "wrong count for {year}");
        }
    }

    // --- Fixed holidays ---

    #[test]
    fn fixed_holidays_present_every_year() {
        for year in 2024..=2030 {
            let holidays = french_holidays(year);
            let dates: Vec<NaiveDate> = holidays.iter().map(|(d, _)| *d).collect();

            assert!(
                dates.contains(&d(year, 1, 1)),
                "missing Jour de l'An {year}"
            );
            assert!(
                dates.contains(&d(year, 5, 1)),
                "missing Fête du Travail {year}"
            );
            assert!(
                dates.contains(&d(year, 5, 8)),
                "missing Victoire 1945 {year}"
            );
            assert!(
                dates.contains(&d(year, 7, 14)),
                "missing Fête Nationale {year}"
            );
            assert!(dates.contains(&d(year, 8, 15)), "missing Assomption {year}");
            assert!(dates.contains(&d(year, 11, 1)), "missing Toussaint {year}");
            assert!(dates.contains(&d(year, 11, 11)), "missing Armistice {year}");
            assert!(dates.contains(&d(year, 12, 25)), "missing Noël {year}");
        }
    }

    // --- Easter-based holidays for specific years ---

    #[test]
    fn easter_based_2026() {
        let holidays = french_holidays(2026);
        let dates: Vec<NaiveDate> = holidays.iter().map(|(d, _)| *d).collect();

        // Easter 2026 = April 5
        assert!(dates.contains(&d(2026, 4, 6)), "missing Lundi de Pâques");
        assert!(dates.contains(&d(2026, 5, 14)), "missing Ascension"); // Easter + 39
        assert!(
            dates.contains(&d(2026, 5, 25)),
            "missing Lundi de Pentecôte"
        ); // Easter + 50
    }

    #[test]
    fn easter_based_2025() {
        let holidays = french_holidays(2025);
        let dates: Vec<NaiveDate> = holidays.iter().map(|(d, _)| *d).collect();

        // Easter 2025 = April 20
        assert!(dates.contains(&d(2025, 4, 21)), "missing Lundi de Pâques");
        assert!(dates.contains(&d(2025, 5, 29)), "missing Ascension");
        assert!(dates.contains(&d(2025, 6, 9)), "missing Lundi de Pentecôte");
    }

    // --- is_holiday ---

    #[test]
    fn is_holiday_returns_name() {
        assert_eq!(is_holiday(d(2026, 5, 1)), Some("Fête du Travail"));
        assert_eq!(is_holiday(d(2026, 12, 25)), Some("Noël"));
        assert_eq!(is_holiday(d(2026, 4, 6)), Some("Lundi de Pâques"));
    }

    #[test]
    fn is_holiday_returns_none_for_regular_day() {
        assert_eq!(is_holiday(d(2026, 4, 16)), None); // random Wednesday
        assert_eq!(is_holiday(d(2026, 6, 15)), None);
    }

    // --- No duplicate dates in a year ---

    #[test]
    fn no_duplicate_dates() {
        for year in 2024..=2035 {
            let holidays = french_holidays(year);
            let dates: Vec<NaiveDate> = holidays.iter().map(|(d, _)| *d).collect();
            let mut deduped = dates.clone();
            deduped.sort();
            deduped.dedup();
            assert_eq!(
                dates.len(),
                deduped.len(),
                "duplicate holiday dates in {year}"
            );
        }
    }
}
