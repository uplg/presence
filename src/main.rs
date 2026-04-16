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
        "Feuille de présence envoyée.\nDestinataires: {}, {}",
        cfg.recipient_1, cfg.recipient_2
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

    sched
        .add(Job::new_async("0 10 17 * * FRI", move |_uuid, _lock| {
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
        })?)
        .await?;

    sched.start().await?;
    info!("scheduler running. Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    info!("shutting down");
    Ok(())
}
