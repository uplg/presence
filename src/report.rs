use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use crate::holidays;

/// What the LLM returns per worked day — just the variable parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaySchedule {
    pub lunch_start: String, // e.g. "12h30"
    pub lunch_end: String,   // e.g. "13h30"
}

/// Full generated report for one day.
#[derive(Debug, Clone)]
pub struct DayReport {
    pub date: NaiveDate,
    pub weekday_name: &'static str,
    pub holiday: Option<&'static str>,
    pub schedule: Option<DaySchedule>, // None if holiday
}

#[derive(Debug, Clone)]
pub struct WeekReport {
    pub days: Vec<DayReport>,
    pub total_hours: u32,
    pub holiday_hours: u32,
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

/// Returns (monday, list of (date, weekday_name, Option<holiday_name>)) for the week containing `date`.
pub fn week_days(date: NaiveDate) -> Vec<(NaiveDate, &'static str, Option<&'static str>)> {
    let weekday_num = date.weekday().num_days_from_monday();
    let monday = date - Duration::days(weekday_num as i64);

    (0..5)
        .map(|i| {
            let day = monday + Duration::days(i);
            let holiday = holidays::is_holiday(day);
            (day, weekday_name(day.weekday()), holiday)
        })
        .collect()
}

/// Count how many worked (non-holiday) days this week.
pub fn worked_day_count(date: NaiveDate) -> usize {
    week_days(date)
        .iter()
        .filter(|(_, _, h)| h.is_none())
        .count()
}

/// Build the prompt that asks the LLM to produce JSON with variable lunch breaks.
pub fn build_llm_prompt(date: NaiveDate) -> String {
    let days = week_days(date);
    let worked: Vec<&str> = days
        .iter()
        .filter(|(_, _, h)| h.is_none())
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
pub fn assemble(date: NaiveDate, llm_output: &LlmWeekOutput) -> WeekReport {
    let days_info = week_days(date);
    let mut schedule_iter = llm_output.days.iter();
    let mut total_hours = 0u32;
    let mut holiday_hours = 0u32;

    let days: Vec<DayReport> = days_info
        .into_iter()
        .map(|(d, name, holiday)| {
            if holiday.is_some() {
                holiday_hours += 7;
                DayReport {
                    date: d,
                    weekday_name: name,
                    holiday,
                    schedule: None,
                }
            } else {
                total_hours += 7;
                let sched = schedule_iter.next().cloned().unwrap_or(DaySchedule {
                    lunch_start: "12h30".into(),
                    lunch_end: "13h30".into(),
                });
                DayReport {
                    date: d,
                    weekday_name: name,
                    holiday: None,
                    schedule: Some(sched),
                }
            }
        })
        .collect();

    WeekReport {
        days,
        total_hours,
        holiday_hours,
    }
}

impl WeekReport {
    /// Format to the exact mail body template.
    pub fn to_mail_body(&self) -> String {
        let mut out = String::new();
        let month_name = french_month(self.days[0].date.month());

        for day in &self.days {
            if let Some(holiday_name) = day.holiday {
                out.push_str(&format!(
                    "{} {} {} - Férié ({}). Temps de travail journalier : 0h. \n",
                    day.weekday_name,
                    day.date.day(),
                    month_name,
                    holiday_name,
                ));
            } else if let Some(sched) = &day.schedule {
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
        out.push_str(&format!(
            "\nTOTAL DURÉE DE TRAVAIL HEBDOMADAIRE : {}h\n",
            self.total_hours
        ));
        out.push_str(&format!(
            "TOTAL CONGES HEBDOMADAIRE : {}h \n",
            self.holiday_hours
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
        let days = week_days(d(2026, 4, 16));
        assert_eq!(days.len(), 5);
        assert_eq!(days[0].1, "Lundi");
        assert_eq!(days[4].1, "Vendredi");
        assert_eq!(days[0].0, d(2026, 4, 13)); // Monday
        assert_eq!(days[4].0, d(2026, 4, 17)); // Friday
    }

    #[test]
    fn week_days_from_friday_same_week() {
        // Friday April 17 2026
        let days = week_days(d(2026, 4, 17));
        assert_eq!(days[0].0, d(2026, 4, 13)); // still Monday of same week
    }

    #[test]
    fn week_days_from_monday() {
        let days = week_days(d(2026, 4, 13));
        assert_eq!(days[0].0, d(2026, 4, 13));
    }

    #[test]
    fn week_days_detects_holiday() {
        // Week of May 1 2026 (Friday)
        let days = week_days(d(2026, 5, 1));
        // May 1 is Fête du Travail (Friday = index 4)
        assert!(days[4].2.is_some());
        assert_eq!(days[4].2.unwrap(), "Fête du Travail");
        // Other days should not be holidays
        assert!(days[0].2.is_none());
    }

    // --- worked_day_count ---

    #[test]
    fn worked_day_count_normal_week() {
        // Week of April 13 2026 — no holidays
        assert_eq!(worked_day_count(d(2026, 4, 16)), 5);
    }

    #[test]
    fn worked_day_count_with_holiday() {
        // Week of May 1 2026 — Fête du Travail on Friday
        assert_eq!(worked_day_count(d(2026, 5, 1)), 4);
    }

    #[test]
    fn worked_day_count_ascension_week_2026() {
        // Ascension 2026 = May 14 (Thursday)
        assert_eq!(worked_day_count(d(2026, 5, 14)), 4);
    }

    // --- build_llm_prompt ---

    #[test]
    fn prompt_contains_day_count() {
        let prompt = build_llm_prompt(d(2026, 4, 16));
        assert!(prompt.contains("5 jours"));
    }

    #[test]
    fn prompt_excludes_holiday_days() {
        // May 1 2026 is Friday (Fête du Travail)
        let prompt = build_llm_prompt(d(2026, 5, 1));
        assert!(prompt.contains("4 jours"));
        assert!(!prompt.contains("Vendredi")); // holiday day excluded from worked list
    }

    #[test]
    fn prompt_contains_valid_pairs_example() {
        let prompt = build_llm_prompt(d(2026, 4, 16));
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
        let week = assemble(d(2026, 4, 16), &schedule);
        assert_eq!(week.days.len(), 5);
        assert_eq!(week.total_hours, 35);
        assert_eq!(week.holiday_hours, 0);
        assert!(week.days.iter().all(|d| d.holiday.is_none()));
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
        let week = assemble(d(2026, 5, 1), &schedule);
        assert_eq!(week.total_hours, 28); // 4 * 7
        assert_eq!(week.holiday_hours, 7);
        // Friday (index 4) should be holiday
        assert!(week.days[4].holiday.is_some());
        assert!(week.days[4].schedule.is_none());
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
        let week = assemble(d(2026, 4, 16), &schedule);
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
        let week = assemble(d(2026, 5, 1), &schedule);
        let body = week.to_mail_body();

        assert!(body.contains("Férié"));
        assert!(body.contains("Fête du Travail"));
        assert!(body.contains("TOTAL DURÉE DE TRAVAIL HEBDOMADAIRE : 28h"));
        assert!(body.contains("TOTAL CONGES HEBDOMADAIRE : 7h"));
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
        let week = assemble(d(2026, 4, 16), &schedule);
        let body = week.to_mail_body();

        // Must NOT contain greetings or signature
        assert!(!body.contains("Bonjour"));
        assert!(!body.contains("Cordialement"));
        assert!(!body.contains("Bonne"));
        // Must start directly with "Lundi"
        assert!(body.starts_with("Lundi"));
    }

    #[test]
    fn mail_body_uses_correct_month_across_boundary() {
        // Week containing Jan 1 2027 (Friday) — the month used should be from Monday Dec 28
        let schedule = make_schedule(&[
            ("12h00", "13h00"),
            ("12h30", "13h30"),
            ("13h00", "14h00"),
            ("13h30", "14h30"),
        ]);
        let week = assemble(d(2027, 1, 1), &schedule);
        let body = week.to_mail_body();
        // Month is derived from first day (Monday Dec 28)
        assert!(body.contains("décembre"));
    }
}
