# Presence — Design notes

## Problem
Every Friday, send a standardized time report email (9h-17h, 7h/day, 35h/week) to two recipients via Gmail, accounting for French public holidays and personal off days, with lunch breaks that vary naturally so the report doesn't look automated.

## Architecture

```
┌──────────────────┐     ┌──────────────────────┐     ┌──────────┐     ┌──────────┐
│ Daemon loop      │────▶│ Report               │────▶│  Gmail   │────▶│   ntfy   │
│ (Fri 17:10-17:30,│     │ holidays + off days  │     │  SMTP    │     │ confirm  │
│  wall-clock poll)│     │ + random lunches     │     └────┬─────┘     └──────────┘
└──────────────────┘     └──────────────────────┘          │
         ▲                                            ~/.presence/sent.json
         └──────────── missed-week check ◀─────────────────┘
```

## Modules

```
src/
├── main.rs      # CLI parsing, send pipeline, daemon loop (next_action is pure + tested)
├── config.rs    # .env loading, ~/.presence data dir
├── holidays.rs  # French public holidays (fixed + Easter-based, computed)
├── off_days.rs  # ~/.presence/off_days.json (congés), CLI-managed
├── ledger.rs    # ~/.presence/sent.json — weeks already emailed (keyed by Monday)
├── report.rs    # Week assembly, lunch draw, mail formatting
├── email.rs     # Gmail SMTP via lettre (native-tls)
└── notify.rs    # ntfy JSON API, plain text
```

## Lunch breaks

No LLM: the old model (Qwen3-0.6B via mistral.rs) only picked one of 5 enum values under a JSON-schema constraint, then got post-corrected. A weighted draw does the same job deterministically, in microseconds, without a 1.2 GB download or GPU memory.

- Slots (always 1h): 12h00, 12h30, 13h00, 13h30, 14h00 — weights 3/4/3/2/1
- Never the same slot 3 days in a row
- Never the same slot for the whole week (2+ worked days)

## Scheduling

- Due time: Friday 17:10 Europe/Paris + random 0-20 min (redrawn each week)
- The daemon polls the wall clock every 60 s: monotonic timers stop during macOS sleep, so a single long sleep would drift
- A late send is allowed on Friday only (Mac woke up in the evening)
- Past Friday with no send recorded → ntfy alert with `presence --now --for <monday>`; nothing is sent automatically after Friday
- Fresh install (empty ledger) never raises a missed-week alert

## Safety

- A week is never emailed twice (`--force` to override)
- Unreadable off days file → the send fails and alerts rather than reporting worked hours on a day off
- Once the email is out, ledger/ntfy failures are logged but never trigger a resend
- Unknown CLI arguments are rejected instead of falling back to daemon mode

## Deployment

The binary is installed with `cargo install --path . --locked` into `~/.cargo/bin` and the LaunchAgent points there, so cleaning `target/` cannot silently kill the daemon (what happened in September 2026).
