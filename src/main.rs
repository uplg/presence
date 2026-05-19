mod config;
mod email;
mod holidays;
mod llm;
mod off_days;
mod report;
mod telegram;

use anyhow::{Context, Result, bail};
use chrono::{Local, NaiveDate};
use rand::RngExt;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info};

/// Run the full pipeline for the week containing `target`:
/// LLM → assemble report → format → send email → notify Telegram.
async fn run_pipeline(cfg: &config::Config, llm: &llm::Llm, target: NaiveDate) -> Result<()> {
    let personal_off = off_days::load_expanded().unwrap_or_else(|e| {
        error!("failed to load off_days.json, treating week as fully worked: {e:#}");
        Vec::new()
    });

    info!("generating schedule for week of {target}...");
    let prompt = report::build_llm_prompt(target, &personal_off);
    let worked_count = report::worked_day_count(target, &personal_off);

    let schedule = llm.generate_schedule(&prompt, worked_count).await?;

    let week = report::assemble(target, &personal_off, &schedule);
    let mail_body = week.to_mail_body();
    info!("mail body:\n{mail_body}");

    let subject = "Feuille de présence";

    info!("sending email...");
    email::send(
        &cfg.gmail_address,
        &cfg.sender_name,
        &cfg.gmail_app_password,
        &[cfg.recipient_1.as_str(), cfg.recipient_2.as_str()],
        subject,
        &mail_body,
    )
    .await?;

    info!("notifying Telegram...");
    let tg_msg = format!(
        "Feuille de présence envoyée.\nDestinataires: {}, {}\n\n{}",
        cfg.recipient_1, cfg.recipient_2, mail_body
    );
    telegram::notify(&cfg.telegram_bot_token, &cfg.telegram_chat_id, &tg_msg).await?;

    info!("done");
    Ok(())
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .with_context(|| format!("invalid date '{s}', expected YYYY-MM-DD"))
}

/// Extract an optional `--for YYYY-MM-DD` (alias `--week`) target date from args.
/// Any day within the desired week works — the report week is derived from it.
/// Used to re-run a missed/failed past week.
fn parse_target_date(args: &[String]) -> Result<Option<NaiveDate>> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--for" || a == "--week" {
            let val = it
                .next()
                .with_context(|| format!("{a} requires a date argument (YYYY-MM-DD)"))?;
            return Ok(Some(parse_date(val)?));
        }
    }
    Ok(None)
}

/// Load the LLM and run the full pipeline for the week containing `target`.
async fn run_once(cfg: &config::Config, target: NaiveDate) -> Result<()> {
    let llm = llm::Llm::load().await.context("LLM load failed")?;
    run_pipeline(cfg, &llm, target).await
}

/// Run for `target`'s week; on ANY failure (LLM load, email, Telegram, …)
/// log it and push a Telegram alert containing the exact recovery command.
async fn run_and_alert(cfg: &config::Config, target: NaiveDate) {
    if let Err(e) = run_once(cfg, target).await {
        error!("run failed for week of {target}: {e:#}");
        let msg = format!(
            "⚠️ Presence : échec de l'envoi de la feuille de présence (semaine du {target}).\n\n\
             Erreur : {e:#}\n\n\
             Pour relancer manuellement cette semaine :\n\
             presence --now --for {target}"
        );
        if let Err(te) =
            telegram::notify_error(&cfg.telegram_bot_token, &cfg.telegram_chat_id, &msg).await
        {
            error!("failed to send Telegram error alert: {te:#}");
        }
    }
}

fn print_usage_off() {
    eprintln!("usage:");
    eprintln!("  presence off add YYYY-MM-DD [YYYY-MM-DD]   add a single day or inclusive range");
    eprintln!("  presence off remove YYYY-MM-DD             remove the entry starting on that date");
    eprintln!("  presence off list                          list configured off days");
}

fn handle_off(args: &[String]) -> Result<()> {
    let Some(sub) = args.first() else {
        print_usage_off();
        bail!("missing subcommand");
    };

    match sub.as_str() {
        "add" => {
            let start = args
                .get(1)
                .with_context(|| "missing start date")
                .and_then(|s| parse_date(s))?;
            let end = match args.get(2) {
                Some(s) => parse_date(s)?,
                None => start,
            };
            let entry = off_days::OffEntry::range(start, end)?;
            let added = off_days::add(entry.clone())?;
            if added {
                if entry.start == entry.end {
                    println!("added: {}", entry.start);
                } else {
                    println!("added: {} → {}", entry.start, entry.end);
                }
            } else {
                println!("already present, no change");
            }
        }
        "remove" | "rm" => {
            let date = args
                .get(1)
                .with_context(|| "missing date")
                .and_then(|s| parse_date(s))?;
            let removed = off_days::remove_by_start(date)?;
            if removed {
                println!("removed entry starting {date}");
            } else {
                println!("no entry starts on {date}");
            }
        }
        "list" | "ls" => {
            let entries = off_days::load()?;
            if entries.is_empty() {
                println!("no off days configured");
            } else {
                for e in entries {
                    if e.start == e.end {
                        println!("{}", e.start);
                    } else {
                        println!("{} → {}", e.start, e.end);
                    }
                }
            }
        }
        other => {
            print_usage_off();
            bail!("unknown subcommand: {other}");
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "presence=info".parse().unwrap()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    // Subcommand: off
    if args.get(1).map(String::as_str) == Some("off") {
        return handle_off(&args[2..]);
    }

    let cfg = config::Config::from_env()?;
    info!("config loaded");

    // Optional `--for YYYY-MM-DD` overrides the target week (defaults to today's
    // week). Lets you re-run a missed/failed past week.
    let target = parse_target_date(&args)?.unwrap_or_else(|| Local::now().date_naive());

    // --now: run immediately and exit. Still alerts on Telegram if it fails.
    if args.iter().any(|a| a == "--now") {
        info!("--now: running immediately for week of {target}");
        run_and_alert(&cfg, target).await;
        return Ok(());
    }

    // --dry-run: generate and print, don't send
    if args.iter().any(|a| a == "--dry-run") {
        info!("--dry-run: generating report only for week of {target}");
        let llm = llm::Llm::load().await?;
        let personal_off = off_days::load_expanded().unwrap_or_else(|e| {
            error!("failed to load off_days.json: {e:#}");
            Vec::new()
        });
        let prompt = report::build_llm_prompt(target, &personal_off);
        let worked_count = report::worked_day_count(target, &personal_off);
        let schedule = llm.generate_schedule(&prompt, worked_count).await?;
        let week = report::assemble(target, &personal_off, &schedule);
        println!("{}", week.to_mail_body());
        return Ok(());
    }

    // Daemon mode: schedule every Friday at 17:10 + random 0-20min delay
    info!("starting scheduler (every Friday 17:10-17:30)...");

    let sched = JobScheduler::new().await?;
    let cfg = Arc::new(cfg);
    let cfg_clone = cfg.clone();

    let job = Job::new_async_tz("0 10 17 * * FRI", chrono_tz::Europe::Paris, move |_uuid, _lock| {
            let cfg = cfg_clone.clone();
            Box::pin(async move {
                let delay = rand::rng().random_range(0u64..1200);
                info!("Friday trigger, delaying {delay}s...");
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;

                run_and_alert(&cfg, Local::now().date_naive()).await;
            })
        })?;
    sched.add(job).await?;

    sched.start().await?;
    info!("scheduler running. Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    info!("shutting down");
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, NaiveTime, TimeZone, Timelike};
    use chrono_tz::Europe::Paris;

    /// Verify that 17:10 Europe/Paris maps to the correct UTC hour,
    /// accounting for CET (winter, UTC+1) vs CEST (summer, UTC+2).
    #[test]
    fn friday_1710_paris_to_utc_summer() {
        // 2026-06-19 is a Friday in summer (CEST, UTC+2)
        let paris_dt = Paris
            .from_local_datetime(
                &NaiveDate::from_ymd_opt(2026, 6, 19)
                    .unwrap()
                    .and_time(NaiveTime::from_hms_opt(17, 10, 0).unwrap()),
            )
            .unwrap();
        let utc = paris_dt.with_timezone(&chrono::Utc);
        assert_eq!(utc.time().hour(), 15, "17:10 CEST should be 15:10 UTC");
        assert_eq!(utc.time().minute(), 10);
    }

    #[test]
    fn friday_1710_paris_to_utc_winter() {
        // 2026-01-16 is a Friday in winter (CET, UTC+1)
        let paris_dt = Paris
            .from_local_datetime(
                &NaiveDate::from_ymd_opt(2026, 1, 16)
                    .unwrap()
                    .and_time(NaiveTime::from_hms_opt(17, 10, 0).unwrap()),
            )
            .unwrap();
        let utc = paris_dt.with_timezone(&chrono::Utc);
        assert_eq!(utc.time().hour(), 16, "17:10 CET should be 16:10 UTC");
        assert_eq!(utc.time().minute(), 10);
    }

    #[test]
    fn paris_offset_differs_summer_vs_winter() {
        let summer = Paris
            .from_local_datetime(
                &NaiveDate::from_ymd_opt(2026, 7, 1)
                    .unwrap()
                    .and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap()),
            )
            .unwrap();
        let winter = Paris
            .from_local_datetime(
                &NaiveDate::from_ymd_opt(2026, 12, 1)
                    .unwrap()
                    .and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap()),
            )
            .unwrap();

        let summer_utc = summer.with_timezone(&chrono::Utc);
        let winter_utc = winter.with_timezone(&chrono::Utc);

        // Summer: UTC+2 → 12h Paris = 10h UTC
        // Winter: UTC+1 → 12h Paris = 11h UTC
        assert_eq!(summer_utc.time().hour(), 10);
        assert_eq!(winter_utc.time().hour(), 11);
    }
}
