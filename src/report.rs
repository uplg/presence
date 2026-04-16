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
