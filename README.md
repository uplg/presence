# Presence

Automated weekly time report. Every Friday between 17:10 and 17:30 (Europe/Paris) it emails a work-hours report (9h-17h, 7h/day, 35h/week) with naturally varied lunch breaks via Gmail SMTP, then confirms on Telegram.

French public holidays are detected automatically ("Férié") and personal off days ("Congé") can be declared from the CLI. Both are deducted from the weekly total.

## Requirements

- macOS (any Mac; nothing is GPU- or model-dependent)
- Rust toolchain (2024 edition)
- A Gmail account with an [App Password](https://support.google.com/accounts/answer/185833)
- A Telegram bot token ([create one via BotFather](https://core.telegram.org/bots#botfather))

## Setup

```bash
git clone https://github.com/uplg/presence.git
cd presence
cp .env.example .env
```

Edit `.env` with your credentials:

```env
GMAIL_ADDRESS=you@company.com
GMAIL_APP_PASSWORD=xxxx-xxxx-xxxx-xxxx
RECIPIENT_1=boss1@company.com
RECIPIENT_2=boss2@company.com
TELEGRAM_BOT_TOKEN=123456:ABC-DEF...
TELEGRAM_CHAT_ID=123456789
SENDER_NAME="Prénom Nom"
```

> `SENDER_NAME` must be quoted if it contains non-ASCII characters.

## Install

```bash
cargo install --path . --locked
```

This puts the binary in `~/.cargo/bin/presence`, outside the repo: `cargo clean` or any `target/` cleanup cannot break the daemon anymore.

## Usage

```bash
# Daemon mode — sends every Friday at 17:10 + random 0-20 min
presence

# Send now for the current week, then exit
presence --now

# Print the report without sending anything (no .env needed)
presence --dry-run

# Send a missed past week — any day of that week works
presence --now --for 2026-09-14
presence --dry-run --for 2026-09-14

# A week is never sent twice; override on purpose with --force
presence --now --for 2026-09-14 --force
```

Unknown arguments are rejected (a typo never silently starts the daemon).

## Personal off days (congés)

```bash
presence off add 2026-05-15              # single day
presence off add 2026-07-13 2026-07-31   # inclusive range
presence off list
presence off remove 2026-05-15           # removes the entry starting that day
```

Entries live in `~/.presence/off_days.json`. The daemon re-reads it at every send, so changes apply without a restart. If the file is unreadable the send fails and alerts instead of reporting worked hours on a day off.

Public holidays take precedence: a day that is both shows as "Férié (<name>)".

## Reliability

- Sent weeks are recorded in `~/.presence/sent.json`, which prevents double sends.
- The daemon checks the wall clock every minute, so a Mac that wakes up after 17:10 still sends that Friday evening.
- If a Friday went by without a send (Mac off, daemon stopped…), the daemon alerts on Telegram as soon as it runs again, with the exact `presence --now --for <date>` command to recover.
- Any failure during a send (SMTP, off days file…) triggers the same Telegram alert.

## Run as a macOS LaunchAgent

```bash
cp com.presence.daemon.plist ~/Library/LaunchAgents/
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.presence.daemon.plist
```

The daemon starts on login and restarts if it crashes. It reads `.env` from the repo (`WorkingDirectory` in the plist).

Logs: `~/Library/Logs/presence.log` and `~/Library/Logs/presence.err`.

After a rebuild:

```bash
cargo install --path . --locked
launchctl kickstart -k gui/$(id -u)/com.presence.daemon
```

To stop:

```bash
launchctl bootout gui/$(id -u)/com.presence.daemon
```

## How it works

1. On Friday, lunch breaks are drawn for each worked day: 1h breaks starting between 12h00 and 14h00, weighted towards usual times, never the same slot 3 days in a row, never the same slot all week.
2. The week is assembled with public holidays ("Férié") and personal off days ("Congé").
3. The report is sent via Gmail SMTP (STARTTLS + App Password) and recorded as sent.
4. A Telegram message confirms delivery.

## Email format

```
Lundi 13 avril - Durée de ma journée de travail : 9h à 17h. Pause déjeuner entre 12h00 et 13h00. Temps de travail journalier : 7h.
Mardi 14 avril - Durée de ma journée de travail : 9h à 17h. Pause déjeuner entre 12h30 et 13h30. Temps de travail journalier : 7h.
...

TOTAL DURÉE DE TRAVAIL HEBDOMADAIRE : 35h
TOTAL CONGES HEBDOMADAIRE : 0h
```

Holidays: `Vendredi 1 mai - Férié (Fête du Travail). Temps de travail journalier : 0h.`
Personal off days: `Vendredi 15 mai - Congé. Temps de travail journalier : 0h.`

## License

MIT
