mod config;
mod email;
mod holidays;
mod ledger;
mod off_days;
mod report;
mod notify;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::{Europe::Paris, Tz};
use rand::RngExt;
use tracing::{error, info, warn};

const SUBJECT: &str = "Feuille de présence";

/// Reports go out on Friday at 17:10 (Europe/Paris) plus a random delay.
const SEND_TIME: NaiveTime = NaiveTime::from_hms_opt(17, 10, 0).unwrap();
const MAX_JITTER_SECS: i64 = 20 * 60;

/// How often the daemon re-checks the wall clock. Polling (rather than one
/// long sleep) keeps the schedule correct across macOS sleep/wake, during
/// which monotonic timers stop.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

fn now_paris() -> DateTime<Tz> {
    Utc::now().with_timezone(&Paris)
}

/// Build the report for the week containing `target`, with fresh lunch breaks.
fn build_report(target: NaiveDate) -> Result<report::WeekReport> {
    // Fail rather than guess: a missing congé would claim worked hours.
    let personal_off = off_days::load_expanded().context("failed to load off days")?;
    Ok(report::generate(target, &personal_off, &mut rand::rng()))
}

/// Build, email and record the report for the week containing `target`, then
/// confirm on ntfy. Refuses to send a week twice unless `force`.
async fn send_week(cfg: &config::Config, target: NaiveDate, force: bool) -> Result<()> {
    let monday = report::week_monday(target);
    if !force && ledger::load()?.contains(&monday) {
        bail!("week of {monday} was already sent (use --force to send it again)");
    }

    let mail_body = build_report(target)?.to_mail_body();
    info!("mail body for week of {monday}:\n{mail_body}");

    info!("sending email...");
    email::send(
        &cfg.gmail_address,
        &cfg.sender_name,
        &cfg.gmail_app_password,
        &[cfg.recipient_1.as_str(), cfg.recipient_2.as_str()],
        SUBJECT,
        &mail_body,
    )
    .await?;

    // The email is out: from here on, failures must not trigger a resend.
    let mut confirmation = format!(
        "Feuille de présence envoyée (semaine du {monday}).\nDestinataires : {}, {}\n\n{mail_body}",
        cfg.recipient_1, cfg.recipient_2
    );
    if let Err(e) = ledger::mark_sent(monday) {
        error!("email sent but failed to record it: {e:#}");
        confirmation.push_str(&format!(
            "\n⚠️ Envoi non enregistré dans le registre : {e:#}"
        ));
    }
    info!("notifying ntfy...");
    if let Err(e) = notify::notify(
        &cfg.ntfy_topic_url,
        &cfg.ntfy_token,
        "Feuille de présence envoyée",
        &confirmation,
        false,
    )
    .await
    {
        error!("email sent but the ntfy confirmation failed: {e:#}");
    }

    info!("done");
    Ok(())
}

/// Send `target`'s week; on ANY failure log it and push an ntfy alert
/// containing the exact recovery command. Returns whether it succeeded.
async fn send_week_or_alert(cfg: &config::Config, target: NaiveDate, force: bool) -> bool {
    let Err(e) = send_week(cfg, target, force).await else {
        return true;
    };
    let monday = report::week_monday(target);
    error!("run failed for week of {monday}: {e:#}");
    alert(
        cfg,
        &format!(
            "⚠️ Presence : échec de l'envoi de la feuille de présence (semaine du {monday}).\n\n\
             Erreur : {e:#}\n\n\
             Pour relancer manuellement cette semaine :\n\
             presence --now --for {monday}"
        ),
    )
    .await;
    false
}

async fn alert(cfg: &config::Config, msg: &str) {
    if let Err(e) = notify::notify(&cfg.ntfy_topic_url, &cfg.ntfy_token, "Presence : action requise", msg, true).await {
        error!("failed to send the ntfy alert: {e:#}");
    }
}

/// When the report of the week starting `monday` is due.
fn due_at(monday: NaiveDate, jitter: Duration) -> DateTime<Tz> {
    let friday = monday + Duration::days(4);
    Paris
        .from_local_datetime(&friday.and_time(SEND_TIME))
        .earliest()
        .expect("Friday 17:10 always exists in Europe/Paris")
        + jitter
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Wait,
    /// It's Friday, the report is due and not sent yet.
    Send(NaiveDate),
    /// The last due Friday went by without a send (machine off, daemon down…).
    Missed(NaiveDate),
}

/// What the daemon should do at `now`, given the weeks already sent and the
/// last week this process already dealt with. Weeks are keyed by Monday.
fn next_action(
    now: DateTime<Tz>,
    jitter: Duration,
    sent: &[NaiveDate],
    handled: Option<NaiveDate>,
) -> Action {
    let this_monday = report::week_monday(now.date_naive());
    let week = if now >= due_at(this_monday, jitter) {
        this_monday
    } else {
        this_monday - Duration::days(7)
    };

    if handled == Some(week) || sent.contains(&week) {
        Action::Wait
    } else if now.date_naive() == week + Duration::days(4) {
        // Late sends (e.g. the Mac woke up at 19:00) are fine, but only on Friday.
        Action::Send(week)
    } else if sent.is_empty() {
        // Fresh install: no history, nothing to catch up on.
        Action::Wait
    } else {
        Action::Missed(week)
    }
}

async fn run_daemon(cfg: &config::Config) -> Result<()> {
    info!("daemon started: reports go out every Friday between 17:10 and 17:30 (Europe/Paris)");
    let mut handled: Option<NaiveDate> = None;
    let mut jitter = (NaiveDate::MIN, Duration::zero());

    loop {
        let now = now_paris();
        let monday = report::week_monday(now.date_naive());
        if jitter.0 != monday {
            jitter = (
                monday,
                Duration::seconds(rand::rng().random_range(0..=MAX_JITTER_SECS)),
            );
            info!(
                "report for week of {monday} due at {}",
                due_at(monday, jitter.1)
            );
        }

        let sent = ledger::load().unwrap_or_else(|e| {
            error!("failed to read sent-weeks ledger: {e:#}");
            Vec::new()
        });

        match next_action(now, jitter.1, &sent, handled) {
            Action::Wait => {}
            Action::Send(week) => {
                handled = Some(week);
                info!("sending report for week of {week}");
                send_week_or_alert(cfg, week, false).await;
            }
            Action::Missed(week) => {
                handled = Some(week);
                warn!("report for week of {week} was never sent");
                alert(
                    cfg,
                    &format!(
                        "⚠️ Presence : la feuille de présence de la semaine du {week} n'a pas été \
                         envoyée vendredi (Mac éteint ou daemon arrêté ?).\n\n\
                         Pour l'envoyer maintenant :\n\
                         presence --now --for {week}"
                    ),
                )
                .await;
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
            _ = tokio::signal::ctrl_c() => {
                info!("shutting down");
                return Ok(());
            }
        }
    }
}

const USAGE: &str = "\
usage:
  presence                               daemon: send every Friday 17:10-17:30 (Europe/Paris)
  presence --now [--for DATE] [--force]  send now for the week containing DATE (default: today)
  presence --dry-run [--for DATE]        print the report without sending anything
  presence off add DATE [END]            add a day off or an inclusive range
  presence off remove DATE               remove the entry starting on DATE
  presence off list                      list configured off days

DATE is YYYY-MM-DD; any day of the wanted week works.";

#[derive(Debug, Default, PartialEq)]
struct Cli {
    now: bool,
    dry_run: bool,
    force: bool,
    target: Option<NaiveDate>,
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .with_context(|| format!("invalid date '{s}', expected YYYY-MM-DD"))
}

/// Parse the flags that follow the program name. Unknown arguments are an
/// error: a typo must never silently fall back to daemon mode.
fn parse_cli(args: &[String]) -> Result<Cli> {
    let mut cli = Cli::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--now" => cli.now = true,
            "--dry-run" => cli.dry_run = true,
            "--force" => cli.force = true,
            "--for" | "--week" => {
                let val = it
                    .next()
                    .with_context(|| format!("{a} requires a date argument (YYYY-MM-DD)"))?;
                cli.target = Some(parse_date(val)?);
            }
            other => bail!("unknown argument: {other}\n\n{USAGE}"),
        }
    }
    if cli.now && cli.dry_run {
        bail!("--now and --dry-run are mutually exclusive");
    }
    if (cli.target.is_some() || cli.force) && !(cli.now || cli.dry_run) {
        bail!("--for and --force only apply to --now or --dry-run");
    }
    Ok(cli)
}

fn handle_off(args: &[String]) -> Result<()> {
    let fmt = |e: &off_days::OffEntry| {
        if e.start == e.end {
            e.start.to_string()
        } else {
            format!("{} → {}", e.start, e.end)
        }
    };

    match args.first().map(String::as_str) {
        Some("add") => {
            let start = parse_date(args.get(1).context("missing start date")?)?;
            let end = args
                .get(2)
                .map(|s| parse_date(s))
                .transpose()?
                .unwrap_or(start);
            let entry = off_days::OffEntry::range(start, end)?;
            if off_days::add(entry.clone())? {
                println!("added: {}", fmt(&entry));
            } else {
                println!("already present, no change");
            }
        }
        Some("remove" | "rm") => {
            let date = parse_date(args.get(1).context("missing date")?)?;
            if off_days::remove_by_start(date)? {
                println!("removed entry starting {date}");
            } else {
                println!("no entry starts on {date}");
            }
        }
        Some("list" | "ls") => {
            let entries = off_days::load()?;
            if entries.is_empty() {
                println!("no off days configured");
            }
            for e in &entries {
                println!("{}", fmt(e));
            }
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "presence=info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("off") => return handle_off(&args[1..]),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            return Ok(());
        }
        _ => {}
    }

    let cli = parse_cli(&args)?;
    let target = cli.target.unwrap_or_else(|| now_paris().date_naive());

    if cli.dry_run {
        print!("{}", build_report(target)?.to_mail_body());
        return Ok(());
    }

    let cfg = config::Config::from_env()?;
    info!("config loaded");

    if cli.now {
        info!(
            "--now: sending report for week of {}",
            report::week_monday(target)
        );
        if !send_week_or_alert(&cfg, target, cli.force).await {
            std::process::exit(1);
        }
        return Ok(());
    }

    run_daemon(&cfg).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn paris(y: i32, m: u32, day: u32, h: u32, min: u32) -> DateTime<Tz> {
        Paris.with_ymd_and_hms(y, m, day, h, min, 0).unwrap()
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    // --- due_at ---

    #[test]
    fn due_at_is_friday_1710_paris_in_summer() {
        // 2026-06-15 is a Monday; Friday 19th is CEST (UTC+2).
        let due = due_at(d(2026, 6, 15), Duration::zero());
        assert_eq!(due, paris(2026, 6, 19, 17, 10));
        assert_eq!(
            due.with_timezone(&Utc),
            Utc.with_ymd_and_hms(2026, 6, 19, 15, 10, 0).unwrap()
        );
    }

    #[test]
    fn due_at_is_friday_1710_paris_in_winter() {
        // 2026-01-12 is a Monday; Friday 16th is CET (UTC+1).
        let due = due_at(d(2026, 1, 12), Duration::zero());
        assert_eq!(
            due.with_timezone(&Utc),
            Utc.with_ymd_and_hms(2026, 1, 16, 16, 10, 0).unwrap()
        );
    }

    #[test]
    fn due_at_adds_jitter() {
        let due = due_at(d(2026, 9, 14), Duration::minutes(20));
        assert_eq!(due, paris(2026, 9, 18, 17, 30));
    }

    // --- next_action ---

    /// Monday of the week whose Friday is 2026-09-18.
    fn sep14() -> NaiveDate {
        d(2026, 9, 14)
    }

    #[test]
    fn waits_before_friday_send_time() {
        let history = [d(2026, 9, 7)];
        let now = paris(2026, 9, 18, 17, 9);
        assert_eq!(
            next_action(now, Duration::zero(), &history, None),
            Action::Wait
        );
    }

    #[test]
    fn waits_for_jitter() {
        let history = [d(2026, 9, 7)];
        let now = paris(2026, 9, 18, 17, 15);
        assert_eq!(
            next_action(now, Duration::minutes(10), &history, None),
            Action::Wait
        );
    }

    #[test]
    fn sends_once_due_on_friday() {
        let now = paris(2026, 9, 18, 17, 21);
        assert_eq!(
            next_action(now, Duration::minutes(10), &[d(2026, 9, 7)], None),
            Action::Send(sep14())
        );
    }

    #[test]
    fn sends_late_on_friday_evening_after_wake() {
        let now = paris(2026, 9, 18, 23, 50);
        assert_eq!(
            next_action(now, Duration::zero(), &[], None),
            Action::Send(sep14())
        );
    }

    #[test]
    fn does_not_resend_a_sent_week() {
        let now = paris(2026, 9, 18, 17, 30);
        assert_eq!(
            next_action(now, Duration::zero(), &[sep14()], None),
            Action::Wait
        );
    }

    #[test]
    fn does_not_repeat_a_handled_week() {
        let now = paris(2026, 9, 18, 17, 30);
        assert_eq!(
            next_action(now, Duration::zero(), &[], Some(sep14())),
            Action::Wait
        );
    }

    #[test]
    fn flags_missed_week_after_friday() {
        let history = [d(2026, 9, 7)];
        // Saturday after the due Friday.
        let now = paris(2026, 9, 19, 9, 0);
        assert_eq!(
            next_action(now, Duration::zero(), &history, None),
            Action::Missed(sep14())
        );
        // Following Wednesday, before that week's Friday.
        let now = paris(2026, 9, 23, 10, 0);
        assert_eq!(
            next_action(now, Duration::zero(), &history, None),
            Action::Missed(sep14())
        );
    }

    #[test]
    fn fresh_install_does_not_flag_missed_week() {
        let now = paris(2026, 9, 23, 10, 0);
        assert_eq!(next_action(now, Duration::zero(), &[], None), Action::Wait);
    }

    // --- parse_cli ---

    #[test]
    fn cli_defaults_to_daemon() {
        assert_eq!(parse_cli(&[]).unwrap(), Cli::default());
    }

    #[test]
    fn cli_now_for_force() {
        let cli = parse_cli(&args(&["--now", "--for", "2026-09-16", "--force"])).unwrap();
        assert_eq!(
            cli,
            Cli {
                now: true,
                dry_run: false,
                force: true,
                target: Some(d(2026, 9, 16)),
            }
        );
    }

    #[test]
    fn cli_week_is_alias_of_for() {
        let cli = parse_cli(&args(&["--dry-run", "--week", "2026-09-16"])).unwrap();
        assert_eq!(cli.target, Some(d(2026, 9, 16)));
    }

    #[test]
    fn cli_rejects_typos_instead_of_starting_daemon() {
        assert!(parse_cli(&args(&["--dryrun"])).is_err());
    }

    #[test]
    fn cli_rejects_conflicting_or_orphan_flags() {
        assert!(parse_cli(&args(&["--now", "--dry-run"])).is_err());
        assert!(parse_cli(&args(&["--for", "2026-09-16"])).is_err());
        assert!(parse_cli(&args(&["--now", "--for"])).is_err());
        assert!(parse_cli(&args(&["--now", "--for", "16/09/2026"])).is_err());
    }
}
