# Presence — Automated Weekly Time Report

## Problem
Every Friday, send a standardized time report email (9h-17h, 7h/day, 35h/week) to two recipients via Gmail, accounting for French public holidays. Use a local LLM to add natural variation to lunch break times so the report doesn't look automated.

## Architecture

```
┌─────────────┐     ┌──────────────┐     ┌─────────────────┐     ┌──────────┐
│  Scheduler   │────▶│ Report Gen   │────▶│  LLM (Gemma 4)  │────▶│  Gmail   │
│ (Fri ~17:15) │     │ + Holidays   │     │  via mistral.rs  │     │  SMTP    │
└─────────────┘     └──────────────┘     └─────────────────┘     └──────────┘
                                                                      │
                                                               ┌──────▼──────┐
                                                               │  Telegram   │
                                                               │  Notifier   │
                                                               └─────────────┘
```

## Stack

| Component       | Crate / Tech                                              |
|-----------------|-----------------------------------------------------------|
| LLM inference   | `mistralrs` 0.8 (Metal acceleration on Apple Silicon)     |
| Model           | `Qwen/Qwen3-0.6B` (Q4 ISQ quantization)                   |
| JSON constraint | `mistralrs::Constraint::JsonSchema` (structured output)   |
| Email           | `lettre` (SMTP + STARTTLS, Gmail App Password)            |
| Telegram        | `reqwest` (Bot API HTTP call)                             |
| Scheduler       | `tokio-cron-scheduler`                                    |
| Date/time       | `chrono`                                                  |
| Config          | `dotenvy` + env vars                                      |
| Async runtime   | `tokio`                                                   |
| Edition         | Rust 2024                                                 |

## Modules

```
src/
├── main.rs          # Entry point, CLI args (--now, --dry-run), cron scheduler
├── config.rs        # Env var loading, typed config struct
├── holidays.rs      # French public holidays for any year (computed algorithmically)
├── report.rs        # Weekly report generation, LLM prompt, JSON schema, fix_pairs(), mail formatting
├── llm.rs           # mistral.rs Gemma 4 E1B inference with JSON schema constraint, retry loop
├── email.rs         # Gmail SMTP sending via lettre
└── telegram.rs      # Telegram bot notification
```

## LLM Details

- **Model**: `Qwen/Qwen3-0.6B` — 0.6B parameter model, very fast locally (~2s inference)
- **Quantization**: Q4 ISQ (In-Situ Quantization by mistral.rs, no pre-quantized files needed)
- **Role**: Generate varied lunch break times as JSON only. No email prose, no greetings.
- **Output constraint**: `Constraint::JsonSchema` forces valid JSON structure
- **Post-processing**: `fix_pairs()` ensures lunch_end = lunch_start + 1h (schema can't enforce pairing)
- **Valid lunch pairs**: 12h00→13h00, 12h30→13h30, 13h00→14h00, 13h30→14h30, 14h00→15h00
- **Retry**: Up to 5 attempts on failure. No random fallback.

## Metal / Build Notes

- Build: `cargo build --release --features metal`
- `MISTRALRS_METAL_PRECOMPILE=0` is set in `.cargo/config.toml` to skip Xcode-dependent shader precompilation
- Shaders compile at runtime via `MTLDevice::newLibraryWithSource` — same kernels, same performance, ~1-2s one-shot cost per process start

## Config (.env)

```env
GMAIL_ADDRESS=you@company.com
GMAIL_APP_PASSWORD=xxxx-xxxx-xxxx-xxxx
RECIPIENT_1=boss1@company.com
RECIPIENT_2=boss2@company.com
TELEGRAM_BOT_TOKEN=123456:ABC-DEF...
TELEGRAM_CHAT_ID=123456789
SENDER_NAME="Prénom Nom"
```

Note: `SENDER_NAME` must be quoted if it contains non-ASCII characters (dotenvy requirement).

## Report Template

No greetings, no signature. Exact format:

```
Lundi 16 mars - Durée de ma journée de travail : 9h à 17h. Pause déjeuner entre 12h30 et 13h30. Temps de travail journalier : 7h.
Mardi 17 mars - Durée de ma journée de travail : 9h à 17h. Pause déjeuner entre 13h00 et 14h00. Temps de travail journalier : 7h.
...

TOTAL DURÉE DE TRAVAIL HEBDOMADAIRE : 35h
TOTAL CONGES HEBDOMADAIRE : 0h
```

Holidays appear as: `Lundi 1 mai - Férié` with 0h, deducted from weekly total.

## French Public Holidays (computed)

Fixed dates + Easter-based (Pâques, Ascension, Pentecôte) calculated algorithmically.

| Holiday              | Rule                    |
|----------------------|-------------------------|
| Jour de l'An         | 1er janvier             |
| Lundi de Pâques      | Easter Monday (computed) |
| Fête du Travail      | 1er mai                 |
| Victoire 1945        | 8 mai                   |
| Ascension            | Easter + 39 days        |
| Lundi de Pentecôte   | Easter + 50 days        |
| Fête Nationale       | 14 juillet              |
| Assomption           | 15 août                 |
| Toussaint            | 1er novembre            |
| Armistice            | 11 novembre             |
| Noël                 | 25 décembre             |

## Scheduler

- Runs every Friday
- Cron triggers at 17:10, then random 0-20min delay (so actual send is 17:10-17:30)
- Flow: load LLM → generate lunch times → assemble report → format mail → send SMTP → notify Telegram

## CLI

```bash
presence              # Daemon mode (cron scheduler)
presence --now        # Run immediately, send email, exit
presence --dry-run    # Generate and print report, don't send
```

## launchd (macOS auto-start)

A plist at `~/Library/LaunchAgents/com.presence.daemon.plist` keeps the daemon running as a user LaunchAgent.
