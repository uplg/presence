mod config;
mod email;
mod holidays;
mod llm;
mod report;
mod telegram;

use anyhow::Result;
use chrono::Local;
use rand::Rng;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info};

/// Run the full pipeline: LLM → assemble report → format → send email → notify Telegram.
async fn run_pipeline(cfg: &config::Config, llm: &llm::Llm) -> Result<()> {
    let today = Local::now().date_naive();

    info!("generating schedule for week of {today}...");
    let prompt = report::build_llm_prompt(today);
    let worked_count = report::worked_day_count(today);

    let schedule = llm.generate_schedule(&prompt, worked_count).await?;

    let week = report::assemble(today, &schedule);
    let mail_body = week.to_mail_body();
    info!("mail body:\n{mail_body}");

    let subject = "Feuille de présence";

    info!("sending email...");
    email::send(
        &cfg.gmail_address,
        &cfg.sender_name,
        &cfg.gmail_app_password,
        &[cfg.recipient_1.as_str(), cfg.recipient_2.as_str()],
        &subject,
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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "presence=info".parse().unwrap()),
        )
        .init();

    let cfg = config::Config::from_env()?;
    info!("config loaded");

    // --now: run immediately and exit
    if std::env::args().any(|a| a == "--now") {
        info!("--now: running immediately");
        let llm = llm::Llm::load().await?;
        return run_pipeline(&cfg, &llm).await;
    }

    // --dry-run: generate and print, don't send
    if std::env::args().any(|a| a == "--dry-run") {
        info!("--dry-run: generating report only");
        let llm = llm::Llm::load().await?;
        let today = Local::now().date_naive();
        let prompt = report::build_llm_prompt(today);
        let worked_count = report::worked_day_count(today);
        let schedule = llm.generate_schedule(&prompt, worked_count).await?;
        let week = report::assemble(today, &schedule);
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

                let llm = match llm::Llm::load().await {
                    Ok(l) => l,
                    Err(e) => {
                        error!("LLM load failed: {e:#}");
                        return;
                    }
                };

                if let Err(e) = run_pipeline(&cfg, &llm).await {
                    error!("pipeline failed: {e:#}");
                }
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
