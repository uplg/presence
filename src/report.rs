use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use crate::holidays;

/// What the LLM returns per worked day — just the variable parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaySchedule {
    pub lunch_start: String, // e.g. "12h30"
    pub lunch_end: String,   // e.g. "13h30"
}

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
    pub schedule: Option<DaySchedule>, // Some only when kind == Worked
}

#[derive(Debug, Clone)]
pub struct WeekReport {
    pub days: Vec<DayReport>,
    pub total_hours: u32,
    pub off_hours: u32, // holidays + personal off days
}

/// The JSON structure we ask the LLM to produce.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmWeekOutput {
    pub days: Vec<DaySchedule>,
}

const VALID_PAIRS: [(&str, &str); 5] = [
    ("12h00", "13h00"),
    ("12h30", "13h30"),
    ("13h00", "14h00"),
    ("13h30", "14h30"),
    ("14h00", "15h00"),
];

impl LlmWeekOutput {
    /// Fix lunch_end to match lunch_start (the JSON schema can't enforce pairing).
    pub fn fix_pairs(&mut self) {
        for day in &mut self.days {
            day.lunch_end = match day.lunch_start.as_str() {
                "12h00" => "13h00",
                "12h30" => "13h30",
                "13h00" => "14h00",
                "13h30" => "14h30",
                "14h00" => "15h00",
                _ => "13h30", // shouldn't happen with schema constraint
            }
            .to_string();
        }
    }

    /// Validate that every entry uses a valid lunch pair and the count matches.
    pub fn validate(&self, expected_days: usize) -> Result<(), String> {
        if self.days.len() != expected_days {
            return Err(format!(
                "expected {} days, got {}",
                expected_days,
                self.days.len()
            ));
        }
        for (i, day) in self.days.iter().enumerate() {
            let pair = (day.lunch_start.as_str(), day.lunch_end.as_str());
            if !VALID_PAIRS.contains(&pair) {
                return Err(format!(
                    "day {}: invalid lunch pair ({}, {})",
                    i + 1,
                    day.lunch_start,
                    day.lunch_end
                ));
            }
        }
        Ok(())
    }
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
    let weekday_num = date.weekday().num_days_from_monday();
    let monday = date - Duration::days(weekday_num as i64);

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

/// Build the prompt that asks the LLM to produce JSON with variable lunch breaks.
pub fn build_llm_prompt(date: NaiveDate, personal_off: &[NaiveDate]) -> String {
    let days = week_days(date, personal_off);
    let worked: Vec<&str> = days
        .iter()
        .filter(|(_, _, k)| matches!(k, DayKind::Worked))
        .map(|(_, name, _)| *name)
        .collect();

    format!(
        r#"Génère un JSON avec des horaires de pause déjeuner pour {count} jours travaillés ({worked_list}).

RÈGLES STRICTES :
- Chaque "lunch_start" DOIT être une de ces 5 valeurs EXACTES : "12h00", "12h30", "13h00", "13h30", "14h00"
- "lunch_end" = lunch_start + 1h. Donc les seules paires valides sont :
  "12h00"→"13h00", "12h30"→"13h30", "13h00"→"14h00", "13h30"→"14h30", "14h00"→"15h00"
- AUCUNE AUTRE VALEUR N'EST ACCEPTÉE
- Varie les paires entre les jours, pas la même tous les jours
- Exactement {count} entrées dans "days"
- UNIQUEMENT du JSON, rien d'autre

Exemple pour 2 jours :
{{"days":[{{"lunch_start":"12h30","lunch_end":"13h30"}},{{"lunch_start":"13h00","lunch_end":"14h00"}}]}}"#,
        worked_list = worked.join(", "),
        count = worked.len(),
    )
}

/// Assemble a WeekReport from the week dates + LLM-generated schedules.
pub fn assemble(
    date: NaiveDate,
    personal_off: &[NaiveDate],
    llm_output: &LlmWeekOutput,
) -> WeekReport {
    let days_info = week_days(date, personal_off);
    let mut schedule_iter = llm_output.days.iter();
    let mut total_hours = 0u32;
    let mut off_hours = 0u32;

    let days: Vec<DayReport> = days_info
        .into_iter()
        .map(|(d, name, kind)| match kind {
            DayKind::Worked => {
                total_hours += 7;
                let sched = schedule_iter.next().cloned().unwrap_or(DaySchedule {
                    lunch_start: "12h30".into(),
                    lunch_end: "13h30".into(),
                });
                DayReport {
                    date: d,
                    weekday_name: name,
                    kind: DayKind::Worked,
                    schedule: Some(sched),
                }
            }
            other => {
                off_hours += 7;
                DayReport {
                    date: d,
                    weekday_name: name,
                    kind: other,
                    schedule: None,
                }
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
                    if let Some(sched) = &day.schedule {
                        out.push_str(&format!(
                            "{} {} {} - Durée de ma journée de travail : 9h à 17h. Pause déjeuner entre {} et {}. Temps de travail journalier : 7h. \n",
                            day.weekday_name,
                            day.date.day(),
                            month_name,
                            sched.lunch_start,
                            sched.lunch_end,
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

    fn make_schedule(pairs: &[(&str, &str)]) -> LlmWeekOutput {
        LlmWeekOutput {
            days: pairs
                .iter()
                .map(|(s, e)| DaySchedule {
                    lunch_start: s.to_string(),
                    lunch_end: e.to_string(),
                })
                .collect(),
        }
    }

    // --- fix_pairs ---

    #[test]
    fn fix_pairs_corrects_mismatched_end() {
        let mut output = make_schedule(&[
            ("12h00", "14h00"), // wrong end
            ("13h30", "13h30"), // wrong end
            ("14h00", "12h00"), // wrong end
        ]);
        output.fix_pairs();
        assert_eq!(output.days[0].lunch_end, "13h00");
        assert_eq!(output.days[1].lunch_end, "14h30");
        assert_eq!(output.days[2].lunch_end, "15h00");
    }

    #[test]
    fn fix_pairs_preserves_correct_pairs() {
        let mut output = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
            ("13h30", "14h30"),
            ("14h00", "15h00"),
        ]);
        output.fix_pairs();
        for (day, (_, expected_end)) in output.days.iter().zip(VALID_PAIRS.iter()) {
            assert_eq!(day.lunch_end, *expected_end);
        }
    }

    #[test]
    fn fix_pairs_handles_unknown_start() {
        let mut output = make_schedule(&[("11h00", "12h00")]);
        output.fix_pairs();
        // Unknown start falls back to "13h30"
        assert_eq!(output.days[0].lunch_end, "13h30");
    }

    // --- validate ---

    #[test]
    fn validate_ok_with_valid_pairs() {
        let output = make_schedule(&[("12h00", "13h00"), ("13h30", "14h30"), ("14h00", "15h00")]);
        assert!(output.validate(3).is_ok());
    }

    #[test]
    fn validate_fails_wrong_count() {
        let output = make_schedule(&[("12h00", "13h00")]);
        let err = output.validate(3).unwrap_err();
        assert!(err.contains("expected 3 days, got 1"));
    }

    #[test]
    fn validate_fails_invalid_pair() {
        let output = make_schedule(&[("12h00", "14h00")]); // bad pairing
        let err = output.validate(1).unwrap_err();
        assert!(err.contains("invalid lunch pair"));
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

    // --- build_llm_prompt ---

    #[test]
    fn prompt_contains_day_count() {
        let prompt = build_llm_prompt(d(2026, 4, 16), &[]);
        assert!(prompt.contains("5 jours"));
    }

    #[test]
    fn prompt_excludes_holiday_days() {
        // May 1 2026 is Friday (Fête du Travail)
        let prompt = build_llm_prompt(d(2026, 5, 1), &[]);
        assert!(prompt.contains("4 jours"));
        assert!(!prompt.contains("Vendredi")); // holiday day excluded from worked list
    }

    #[test]
    fn prompt_excludes_personal_off_days() {
        // Week Apr 13-17 2026 (no holidays). Friday Apr 17 marked as personal off.
        let prompt = build_llm_prompt(d(2026, 4, 13), &[d(2026, 4, 17)]);
        assert!(prompt.contains("4 jours"));
        assert!(!prompt.contains("Vendredi"));
    }

    #[test]
    fn prompt_contains_valid_pairs_example() {
        let prompt = build_llm_prompt(d(2026, 4, 16), &[]);
        assert!(prompt.contains("12h00"));
        assert!(prompt.contains("13h30"));
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
        assert!(week.days.iter().all(|d| d.schedule.is_some()));
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
        assert!(week.days[4].schedule.is_none());
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
        assert!(week.days[4].schedule.is_none());
    }

    #[test]
    fn assemble_with_holiday_and_personal_off() {
        // Week of May 11-15 2026: Thu May 14 = Ascension, Fri May 15 = personal off
        let schedule = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
        ]);
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
        let week = assemble(
            d(2026, 7, 13),
            &off,
            &LlmWeekOutput { days: vec![] },
        );
        assert_eq!(week.total_hours, 0);
        assert_eq!(week.off_hours, 35);
        let body = week.to_mail_body();
        assert!(body.contains("Mardi 14 juillet - Férié (Fête Nationale)"));
        assert_eq!(body.matches("Congé").count(), 4);
    }

    #[test]
    fn fully_off_week_produces_zero_worked_hours() {
        // Range fully covers week of Aug 10-14 2026. LLM gets 0 worked days → empty schedule.
        let off = expand(d(2026, 8, 3), d(2026, 8, 28));
        let schedule = LlmWeekOutput { days: vec![] };
        let week = assemble(d(2026, 8, 12), &off, &schedule);
        assert_eq!(week.total_hours, 0);
        assert_eq!(week.off_hours, 35);
        let body = week.to_mail_body();
        assert!(body.contains("TOTAL DURÉE DE TRAVAIL HEBDOMADAIRE : 0h"));
        assert!(body.contains("TOTAL CONGES HEBDOMADAIRE : 35h"));
        // Every day must be a Congé line, no schedule
        assert_eq!(body.matches("Congé").count(), 5);
    }

    #[test]
    fn prompt_for_fully_off_week_requests_zero_days() {
        let off = expand(d(2026, 8, 3), d(2026, 8, 28));
        let prompt = build_llm_prompt(d(2026, 8, 12), &off);
        assert!(prompt.contains("0 jours"));
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
        let schedule = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
        ]);
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
