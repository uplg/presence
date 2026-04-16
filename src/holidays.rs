use chrono::NaiveDate;

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

use chrono::Datelike;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_easter_2026() {
        // Easter 2026 is April 5
        assert_eq!(easter(2026), NaiveDate::from_ymd_opt(2026, 4, 5).unwrap());
    }

    #[test]
    fn test_holidays_2026_count() {
        assert_eq!(french_holidays(2026).len(), 11);
    }

    #[test]
    fn test_may_1_is_holiday() {
        assert_eq!(
            is_holiday(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()),
            Some("Fête du Travail")
        );
    }
}
