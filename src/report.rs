use chrono::{Datelike, Duration, NaiveDate, Weekday};
use rand::Rng;
use rand::seq::IndexedRandom;

use crate::holidays;

/// A lunch break as (start, end), formatted the way it appears in the mail.
pub type LunchSlot = (&'static str, &'static str);

/// Allowed lunch breaks (always 1h) with their relative weights: early-ish
/// lunches are the most common, 14h00 stays rare.
const LUNCH_SLOTS: [(LunchSlot, u32); 5] = [
    (("12h00", "13h00"), 3),
    (("12h30", "13h30"), 4),
    (("13h00", "14h00"), 3),
    (("13h30", "14h30"), 2),
    (("14h00", "15h00"), 1),
];

/// How a single day is categorised in the weekly report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DayKind {
    Worked,
    Holiday(&'static str),
    Off, // personal off day (congé)
}

/// Full generated report for one day.
#[derive(Debug, Clone)]
pub struct DayReport {
    pub date: NaiveDate,
    pub weekday_name: &'static str,
    pub kind: DayKind,
    pub lunch: Option<LunchSlot>, // Some only when kind == Worked
}

#[derive(Debug, Clone)]
pub struct WeekReport {
    pub days: Vec<DayReport>,
    pub total_hours: u32,
    pub off_hours: u32, // holidays + personal off days
}

/// Draw `count` lunch breaks that look naturally varied:
/// - weighted towards usual lunch times,
/// - never the same slot 3 days in a row,
/// - never the same slot for the whole week (when there are 2+ days).
pub fn lunch_schedule<R: Rng + ?Sized>(count: usize, rng: &mut R) -> Vec<LunchSlot> {
    let mut out: Vec<LunchSlot> = Vec::with_capacity(count);
    for i in 0..count {
        let three_in_a_row = i >= 2 && out[i - 1] == out[i - 2];
        let uniform_week = i >= 1 && i == count - 1 && out.iter().all(|s| *s == out[0]);
        let banned = (three_in_a_row || uniform_week).then(|| out[i - 1]);

        let candidates: Vec<_> = LUNCH_SLOTS
            .iter()
            .filter(|(slot, _)| Some(*slot) != banned)
            .collect();
        let (slot, _) = candidates
            .choose_weighted(rng, |(_, w)| *w)
            .expect("lunch slot weights are positive");
        out.push(*slot);
    }
    out
}

/// Monday of the week containing `date`.
pub fn week_monday(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

fn weekday_name(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Lundi",
        Weekday::Tue => "Mardi",
        Weekday::Wed => "Mercredi",
        Weekday::Thu => "Jeudi",
        Weekday::Fri => "Vendredi",
        Weekday::Sat => "Samedi",
        Weekday::Sun => "Dimanche",
    }
}

fn french_month(m: u32) -> &'static str {
    match m {
        1 => "janvier",
        2 => "février",
        3 => "mars",
        4 => "avril",
        5 => "mai",
        6 => "juin",
        7 => "juillet",
        8 => "août",
        9 => "septembre",
        10 => "octobre",
        11 => "novembre",
        12 => "décembre",
        _ => "???",
    }
}

/// Classify each Mon-Fri of the week containing `date`. Public holidays take
/// precedence over personal off days (so a day that is both still shows the
/// holiday name).
pub fn week_days(
    date: NaiveDate,
    personal_off: &[NaiveDate],
) -> Vec<(NaiveDate, &'static str, DayKind)> {
    let monday = week_monday(date);

    (0..5)
        .map(|i| {
            let day = monday + Duration::days(i);
            let kind = if let Some(name) = holidays::is_holiday(day) {
                DayKind::Holiday(name)
            } else if personal_off.contains(&day) {
                DayKind::Off
            } else {
                DayKind::Worked
            };
            (day, weekday_name(day.weekday()), kind)
        })
        .collect()
}

/// Count how many worked (non-holiday, non-off) days this week.
pub fn worked_day_count(date: NaiveDate, personal_off: &[NaiveDate]) -> usize {
    week_days(date, personal_off)
        .iter()
        .filter(|(_, _, k)| matches!(k, DayKind::Worked))
        .count()
}

/// Build the report for the week containing `date`, drawing fresh lunch breaks.
pub fn generate<R: Rng + ?Sized>(
    date: NaiveDate,
    personal_off: &[NaiveDate],
    rng: &mut R,
) -> WeekReport {
    let lunches = lunch_schedule(worked_day_count(date, personal_off), rng);
    assemble(date, personal_off, &lunches)
}

/// Assemble a WeekReport from the week dates + one lunch slot per worked day.
pub fn assemble(date: NaiveDate, personal_off: &[NaiveDate], lunches: &[LunchSlot]) -> WeekReport {
    let mut lunches = lunches.iter().copied();
    let mut total_hours = 0u32;
    let mut off_hours = 0u32;

    let days: Vec<DayReport> = week_days(date, personal_off)
        .into_iter()
        .map(|(date, weekday_name, kind)| {
            let lunch = if kind == DayKind::Worked {
                total_hours += 7;
                Some(lunches.next().expect("one lunch slot per worked day"))
            } else {
                off_hours += 7;
                None
            };
            DayReport {
                date,
                weekday_name,
                kind,
                lunch,
            }
        })
        .collect();

    WeekReport {
        days,
        total_hours,
        off_hours,
    }
}

impl WeekReport {
    /// Format to the exact mail body template.
    pub fn to_mail_body(&self) -> String {
        let mut out = String::new();

        for day in &self.days {
            let month_name = french_month(day.date.month());
            match &day.kind {
                DayKind::Holiday(name) => {
                    out.push_str(&format!(
                        "{} {} {} - Férié ({}). Temps de travail journalier : 0h. \n",
                        day.weekday_name,
                        day.date.day(),
                        month_name,
                        name,
                    ));
                }
                DayKind::Off => {
                    out.push_str(&format!(
                        "{} {} {} - Congé. Temps de travail journalier : 0h. \n",
                        day.weekday_name,
                        day.date.day(),
                        month_name,
                    ));
                }
                DayKind::Worked => {
                    if let Some((start, end)) = day.lunch {
                        out.push_str(&format!(
                            "{} {} {} - Durée de ma journée de travail : 9h à 17h. Pause déjeuner entre {} et {}. Temps de travail journalier : 7h. \n",
                            day.weekday_name,
                            day.date.day(),
                            month_name,
                            start,
                            end,
                        ));
                    }
                }
            }
        }
        out.push_str(&format!(
            "\nTOTAL DURÉE DE TRAVAIL HEBDOMADAIRE : {}h\n",
            self.total_hours
        ));
        out.push_str(&format!(
            "TOTAL CONGES HEBDOMADAIRE : {}h \n",
            self.off_hours
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn make_schedule(pairs: &[LunchSlot]) -> Vec<LunchSlot> {
        pairs.to_vec()
    }

    fn rng(seed: u64) -> rand::rngs::StdRng {
        rand::SeedableRng::seed_from_u64(seed)
    }

    // --- lunch_schedule ---

    #[test]
    fn lunch_schedule_returns_requested_count_of_valid_slots() {
        for n in 0..=5 {
            let lunches = lunch_schedule(n, &mut rng(n as u64));
            assert_eq!(lunches.len(), n);
            for slot in &lunches {
                assert!(
                    LUNCH_SLOTS.iter().any(|(s, _)| s == slot),
                    "unknown slot {slot:?}"
                );
            }
        }
    }

    #[test]
    fn lunch_breaks_always_last_one_hour() {
        for ((start, end), _) in LUNCH_SLOTS {
            let h = |s: &str| s[..2].parse::<u32>().unwrap() * 60 + s[3..].parse::<u32>().unwrap();
            assert_eq!(h(end) - h(start), 60, "{start} → {end}");
        }
    }

    #[test]
    fn lunch_schedule_never_three_in_a_row() {
        for seed in 0..2000 {
            let lunches = lunch_schedule(5, &mut rng(seed));
            for w in lunches.windows(3) {
                assert!(!(w[0] == w[1] && w[1] == w[2]), "seed {seed}: {lunches:?}");
            }
        }
    }

    #[test]
    fn lunch_schedule_never_uniform_week() {
        for n in 2..=5 {
            for seed in 0..2000 {
                let lunches = lunch_schedule(n, &mut rng(seed));
                assert!(
                    lunches.iter().any(|s| *s != lunches[0]),
                    "seed {seed}: {lunches:?}"
                );
            }
        }
    }

    #[test]
    fn lunch_schedule_uses_every_slot_over_time() {
        let mut seen = std::collections::HashSet::new();
        for seed in 0..200 {
            seen.extend(lunch_schedule(5, &mut rng(seed)));
        }
        assert_eq!(seen.len(), LUNCH_SLOTS.len());
    }

    #[test]
    fn generate_fills_every_worked_day() {
        // Week of May 11 2026: Thu = Ascension, Fri = personal off → 3 worked days.
        let week = generate(d(2026, 5, 12), &[d(2026, 5, 15)], &mut rng(42));
        assert_eq!(week.total_hours, 21);
        assert_eq!(week.off_hours, 14);
        for day in &week.days {
            assert_eq!(day.lunch.is_some(), day.kind == DayKind::Worked);
        }
    }

    // --- week_days ---

    #[test]
    fn week_days_returns_5_days_mon_to_fri() {
        // 2026-04-16 is a Wednesday
        let days = week_days(d(2026, 4, 16), &[]);
        assert_eq!(days.len(), 5);
        assert_eq!(days[0].1, "Lundi");
        assert_eq!(days[4].1, "Vendredi");
        assert_eq!(days[0].0, d(2026, 4, 13)); // Monday
        assert_eq!(days[4].0, d(2026, 4, 17)); // Friday
    }

    #[test]
    fn week_days_from_friday_same_week() {
        // Friday April 17 2026
        let days = week_days(d(2026, 4, 17), &[]);
        assert_eq!(days[0].0, d(2026, 4, 13)); // still Monday of same week
    }

    #[test]
    fn week_days_from_monday() {
        let days = week_days(d(2026, 4, 13), &[]);
        assert_eq!(days[0].0, d(2026, 4, 13));
    }

    #[test]
    fn week_days_detects_holiday() {
        // Week of May 1 2026 (Friday)
        let days = week_days(d(2026, 5, 1), &[]);
        // May 1 is Fête du Travail (Friday = index 4)
        assert_eq!(days[4].2, DayKind::Holiday("Fête du Travail"));
        assert_eq!(days[0].2, DayKind::Worked);
    }

    #[test]
    fn week_days_detects_personal_off() {
        // Week of Apr 13-17 2026 (no holidays). Mark Friday Apr 17 as personal off.
        let days = week_days(d(2026, 4, 13), &[d(2026, 4, 17)]);
        assert_eq!(days[4].2, DayKind::Off);
        assert_eq!(days[0].2, DayKind::Worked);
    }

    #[test]
    fn holiday_takes_precedence_over_personal_off() {
        // May 1 2026 is a holiday; even if marked as off, holiday wins.
        let days = week_days(d(2026, 5, 1), &[d(2026, 5, 1)]);
        assert_eq!(days[4].2, DayKind::Holiday("Fête du Travail"));
    }

    // --- worked_day_count ---

    #[test]
    fn worked_day_count_normal_week() {
        // Week of April 13 2026 — no holidays
        assert_eq!(worked_day_count(d(2026, 4, 16), &[]), 5);
    }

    #[test]
    fn worked_day_count_with_holiday() {
        // Week of May 1 2026 — Fête du Travail on Friday
        assert_eq!(worked_day_count(d(2026, 5, 1), &[]), 4);
    }

    #[test]
    fn worked_day_count_with_personal_off() {
        // Week Apr 13-17 2026 has no holidays. Friday Apr 17 off → 4 worked.
        assert_eq!(worked_day_count(d(2026, 4, 13), &[d(2026, 4, 17)]), 4);
    }

    #[test]
    fn worked_day_count_with_holiday_and_personal_off() {
        // Week of May 11-15 2026: Thu May 14 = Ascension, Fri May 15 = personal off
        assert_eq!(worked_day_count(d(2026, 5, 12), &[d(2026, 5, 15)]), 3);
    }

    #[test]
    fn worked_day_count_ascension_week_2026() {
        // Ascension 2026 = May 14 (Thursday)
        assert_eq!(worked_day_count(d(2026, 5, 14), &[]), 4);
    }

    // --- assemble ---

    #[test]
    fn assemble_normal_week() {
        let schedule = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
            ("13h30", "14h30"),
            ("14h00", "15h00"),
        ]);
        let week = assemble(d(2026, 4, 16), &[], &schedule);
        assert_eq!(week.days.len(), 5);
        assert_eq!(week.total_hours, 35);
        assert_eq!(week.off_hours, 0);
        assert!(week.days.iter().all(|d| d.kind == DayKind::Worked));
        assert!(week.days.iter().all(|d| d.lunch.is_some()));
    }

    #[test]
    fn assemble_with_holiday() {
        // May 1 2026 is Friday — Fête du Travail
        let schedule = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
            ("13h30", "14h30"),
        ]);
        let week = assemble(d(2026, 5, 1), &[], &schedule);
        assert_eq!(week.total_hours, 28); // 4 * 7
        assert_eq!(week.off_hours, 7);
        assert!(matches!(week.days[4].kind, DayKind::Holiday(_)));
        assert!(week.days[4].lunch.is_none());
    }

    #[test]
    fn assemble_with_personal_off() {
        // Week Apr 13-17 2026 (no holidays). Friday Apr 17 personal off.
        let schedule = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
            ("13h30", "14h30"),
        ]);
        let week = assemble(d(2026, 4, 13), &[d(2026, 4, 17)], &schedule);
        assert_eq!(week.total_hours, 28);
        assert_eq!(week.off_hours, 7);
        assert_eq!(week.days[4].kind, DayKind::Off);
        assert!(week.days[4].lunch.is_none());
    }

    #[test]
    fn assemble_with_holiday_and_personal_off() {
        // Week of May 11-15 2026: Thu May 14 = Ascension, Fri May 15 = personal off
        let schedule = make_schedule(&[("12h00", "13h00"), ("12h30", "13h30"), ("13h00", "14h00")]);
        let week = assemble(d(2026, 5, 12), &[d(2026, 5, 15)], &schedule);
        assert_eq!(week.total_hours, 21); // 3 * 7
        assert_eq!(week.off_hours, 14); // 2 * 7
        assert!(matches!(week.days[3].kind, DayKind::Holiday(_)));
        assert_eq!(week.days[4].kind, DayKind::Off);
    }

    // --- to_mail_body ---

    #[test]
    fn mail_body_format_normal_week() {
        let schedule = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
            ("13h30", "14h30"),
            ("14h00", "15h00"),
        ]);
        let week = assemble(d(2026, 4, 16), &[], &schedule);
        let body = week.to_mail_body();

        assert!(body.contains("Lundi 13 avril"));
        assert!(body.contains("Vendredi 17 avril"));
        assert!(body.contains("Pause déjeuner entre 12h00 et 13h00"));
        assert!(body.contains("Temps de travail journalier : 7h."));
        assert!(body.contains("TOTAL DURÉE DE TRAVAIL HEBDOMADAIRE : 35h"));
        assert!(body.contains("TOTAL CONGES HEBDOMADAIRE : 0h"));
    }

    #[test]
    fn mail_body_format_with_holiday() {
        let schedule = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
            ("13h30", "14h30"),
        ]);
        let week = assemble(d(2026, 5, 1), &[], &schedule);
        let body = week.to_mail_body();

        assert!(body.contains("Férié"));
        assert!(body.contains("Fête du Travail"));
        assert!(body.contains("TOTAL DURÉE DE TRAVAIL HEBDOMADAIRE : 28h"));
        assert!(body.contains("TOTAL CONGES HEBDOMADAIRE : 7h"));
    }

    #[test]
    fn mail_body_format_with_personal_off() {
        // Week Apr 13-17 2026 (no holidays). Friday Apr 17 personal off.
        let schedule = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
            ("13h30", "14h30"),
        ]);
        let week = assemble(d(2026, 4, 13), &[d(2026, 4, 17)], &schedule);
        let body = week.to_mail_body();
        assert!(body.contains("Vendredi 17 avril - Congé. Temps de travail journalier : 0h."));
        assert!(body.contains("TOTAL DURÉE DE TRAVAIL HEBDOMADAIRE : 28h"));
        assert!(body.contains("TOTAL CONGES HEBDOMADAIRE : 7h"));
    }

    /// Helper that mimics what off_days::load_expanded() returns from a single range entry.
    fn expand(start: NaiveDate, end: NaiveDate) -> Vec<NaiveDate> {
        let mut out = Vec::new();
        let mut d = start;
        while d <= end {
            out.push(d);
            d += chrono::Duration::days(1);
        }
        out
    }

    #[test]
    fn range_covering_full_week_marks_all_days_off() {
        // 4-week summer range Aug 3 → Aug 28 2026 (no public holiday in window).
        // Pick a fully-covered week (Aug 10-14).
        let off = expand(d(2026, 8, 3), d(2026, 8, 28));
        let days = week_days(d(2026, 8, 12), &off);
        assert!(days.iter().all(|(_, _, k)| *k == DayKind::Off));
        assert_eq!(worked_day_count(d(2026, 8, 12), &off), 0);
    }

    #[test]
    fn range_partial_at_start_of_range() {
        // Range Aug 3 → Aug 28 2026; Aug 3 is a Monday → full week off.
        let off = expand(d(2026, 8, 3), d(2026, 8, 28));
        let days = week_days(d(2026, 8, 3), &off);
        assert!(days.iter().all(|(_, _, k)| *k == DayKind::Off));
    }

    #[test]
    fn range_partial_at_end_of_range() {
        // Range Aug 3 → Aug 28 2026; Aug 28 is a Friday → final week fully off.
        let off = expand(d(2026, 8, 3), d(2026, 8, 28));
        let days = week_days(d(2026, 8, 24), &off);
        assert!(days.iter().all(|(_, _, k)| *k == DayKind::Off));
        // Following week (Aug 31 - Sep 4) should be fully worked.
        let next = week_days(d(2026, 8, 31), &off);
        assert!(next.iter().all(|(_, _, k)| *k == DayKind::Worked));
    }

    #[test]
    fn range_partial_overlap_week_boundary() {
        // Range Wed Aug 5 → Tue Aug 11 2026 — splits across two weeks, no holidays.
        let off = expand(d(2026, 8, 5), d(2026, 8, 11));
        // Week of Aug 3: Mon/Tue worked, Wed/Thu/Fri off
        let w1 = week_days(d(2026, 8, 3), &off);
        assert_eq!(w1[0].2, DayKind::Worked); // Mon 3
        assert_eq!(w1[1].2, DayKind::Worked); // Tue 4
        assert_eq!(w1[2].2, DayKind::Off); // Wed 5
        assert_eq!(w1[3].2, DayKind::Off); // Thu 6
        assert_eq!(w1[4].2, DayKind::Off); // Fri 7
        // Week of Aug 10: Mon/Tue off, Wed/Thu/Fri worked
        let w2 = week_days(d(2026, 8, 10), &off);
        assert_eq!(w2[0].2, DayKind::Off); // Mon 10
        assert_eq!(w2[1].2, DayKind::Off); // Tue 11
        assert_eq!(w2[2].2, DayKind::Worked); // Wed 12
        assert_eq!(w2[3].2, DayKind::Worked); // Thu 13
        assert_eq!(w2[4].2, DayKind::Worked); // Fri 14
    }

    #[test]
    fn range_spanning_public_holiday() {
        // Range Mon Jul 13 → Fri Jul 17 2026 includes Tue 14 (Fête Nationale).
        // The holiday must still display as "Férié", not "Congé".
        let off = expand(d(2026, 7, 13), d(2026, 7, 17));
        let days = week_days(d(2026, 7, 13), &off);
        assert_eq!(days[0].2, DayKind::Off); // Mon 13
        assert_eq!(days[1].2, DayKind::Holiday("Fête Nationale")); // Tue 14
        assert_eq!(days[2].2, DayKind::Off); // Wed 15
        assert_eq!(days[3].2, DayKind::Off); // Thu 16
        assert_eq!(days[4].2, DayKind::Off); // Fri 17
        // Total off hours = 5 * 7 = 35h (4 personal off + 1 holiday)
        let week = assemble(d(2026, 7, 13), &off, &[]);
        assert_eq!(week.total_hours, 0);
        assert_eq!(week.off_hours, 35);
        let body = week.to_mail_body();
        assert!(body.contains("Mardi 14 juillet - Férié (Fête Nationale)"));
        assert_eq!(body.matches("Congé").count(), 4);
    }

    #[test]
    fn fully_off_week_produces_zero_worked_hours() {
        // Range fully covers week of Aug 10-14 2026 → 0 worked days, no lunch slots.
        let off = expand(d(2026, 8, 3), d(2026, 8, 28));
        let week = assemble(d(2026, 8, 12), &off, &[]);
        assert_eq!(week.total_hours, 0);
        assert_eq!(week.off_hours, 35);
        let body = week.to_mail_body();
        assert!(body.contains("TOTAL DURÉE DE TRAVAIL HEBDOMADAIRE : 0h"));
        assert!(body.contains("TOTAL CONGES HEBDOMADAIRE : 35h"));
        // Every day must be a Congé line, no lunch
        assert_eq!(body.matches("Congé").count(), 5);
    }

    #[test]
    fn off_day_in_future_week_does_not_affect_current_week() {
        // We're computing for week of Apr 13 2026, but off day is Jul 17 2026 — unrelated.
        let off = vec![d(2026, 7, 17)];
        let days = week_days(d(2026, 4, 13), &off);
        assert!(days.iter().all(|(_, _, k)| *k == DayKind::Worked));
        assert_eq!(worked_day_count(d(2026, 4, 13), &off), 5);
    }

    #[test]
    fn mail_body_format_with_holiday_and_personal_off() {
        // Week of May 11 2026: Thu = Ascension (férié), Fri = perso off
        let schedule = make_schedule(&[("12h00", "13h00"), ("12h30", "13h30"), ("13h00", "14h00")]);
        let week = assemble(d(2026, 5, 12), &[d(2026, 5, 15)], &schedule);
        let body = week.to_mail_body();
        assert!(body.contains("Jeudi 14 mai - Férié (Ascension)"));
        assert!(body.contains("Vendredi 15 mai - Congé"));
        assert!(body.contains("TOTAL DURÉE DE TRAVAIL HEBDOMADAIRE : 21h"));
        assert!(body.contains("TOTAL CONGES HEBDOMADAIRE : 14h"));
    }

    #[test]
    fn mail_body_no_greetings() {
        let schedule = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
            ("13h30", "14h30"),
            ("14h00", "15h00"),
        ]);
        let week = assemble(d(2026, 4, 16), &[], &schedule);
        let body = week.to_mail_body();

        // Must NOT contain greetings or signature
        assert!(!body.contains("Bonjour"));
        assert!(!body.contains("Cordialement"));
        assert!(!body.contains("Bonne"));
        // Must start directly with "Lundi"
        assert!(body.starts_with("Lundi"));
    }

    #[test]
    fn mail_body_uses_per_day_month_across_boundary() {
        // Week containing Jan 1 2027 (Friday): Mon Dec 28 → Fri Jan 1.
        // Each day must show its own month, not Monday's.
        let schedule = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
            ("13h30", "14h30"),
        ]);
        let week = assemble(d(2027, 1, 1), &[], &schedule);
        let body = week.to_mail_body();
        assert!(body.contains("Lundi 28 décembre"));
        assert!(body.contains("Mardi 29 décembre"));
        assert!(body.contains("Mercredi 30 décembre"));
        assert!(body.contains("Jeudi 31 décembre"));
        assert!(body.contains("Vendredi 1 janvier"));
    }

    #[test]
    fn mail_body_april_to_may_2026_no_april_1_for_labour_day() {
        // Reproduces the prod bug: week of May 1 2026 (Fête du Travail Friday)
        // spans Mon Apr 27 → Fri May 1. May 1 must show "1 mai", not "1 avril".
        let schedule = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
            ("13h30", "14h30"),
        ]);
        let week = assemble(d(2026, 5, 1), &[], &schedule);
        let body = week.to_mail_body();
        assert!(body.contains("Jeudi 30 avril"));
        assert!(body.contains("Vendredi 1 mai"));
        assert!(body.contains("Fête du Travail"));
        assert!(!body.contains("1 avril"));
    }
}
