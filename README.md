# Wellbeing

Wellbeing is a lightweight, self-hosted emotional support companion.

It is built for reflection, check-ins, and grounded conversation. It is not trying to be your work copilot, your doctor, or your therapist.

**Important:** Wellbeing is not a replacement for therapy, professional mental health care, medical advice, legal advice, or emergency support. If someone is in crisis, they need real human help.

## What it does

- runs as a small Rust service
- supports the web portal and Telegram today
- keeps runtime config in `config/config.json`
- gives each tenant its own `agent.md` and `bootstrap.md`
- stores chat, check-in state, and memory in SQLite
- supports personal Gemini BYOK
- applies local guardrails before model calls

## Product boundary

Wellbeing is intentionally narrow.

It should help with emotional support, companionship, reflection, and gentle check-ins. It should not help with work tasks, coding, medical advice, legal advice, or anything that pretends to be clinical care.

## Quick start

Requirements:

- Rust
- a Gemini API key if you want live model responses
- optional `WELLBEING_MASTER_KEY` if you want encrypted BYOK storage

Run it from the project root:

```bash
cargo run
```

Then open `http://127.0.0.1:8080/`.

## How it is structured

- `config/config.json` keeps host and tenant config
- `templates/tenant/agent.md` defines the companion's voice and role
- `templates/tenant/bootstrap.md` sets startup behavior
- `src/guardrails.rs` handles scope checks, prompt assembly, and reply filtering
- `src/companion.rs` handles chat flow and memory refresh
- `src/database.rs` handles SQLite persistence

## Memory and safety

Memory is deliberately simple: recent chat history, rolling summaries, and structured notes for things like preferences, boundaries, people, relationships, and key events.

Safety is layered. Messages are checked before they reach the model, blocked or clarify-only turns stay local, and replies are sanitized on the way out. The goal is simple: keep the companion emotionally supportive without drifting into job-assistant or fake-clinician territory.

## Gemini BYOK

Users can add their own Gemini API key during onboarding or in settings. When `WELLBEING_MASTER_KEY` is set, that key is stored encrypted at rest.

There is also an in-app setup guide at `/gemini-guide.html`.

## Optional signup protection

Wellbeing can protect signup with Cloudflare Turnstile.

- Turnstile is controlled with `WELLBEING_TURNSTILE_SITE_KEY` and `WELLBEING_TURNSTILE_SECRET_KEY`
- email verification is optional and stays off unless you also configure SES on purpose

## Development

Useful commands:

```bash
cargo check
cargo test
python3 tools/prompt_harness.py
```

For more detail, see `docs/development.md`.

## Contributing

If you work on Wellbeing, please keep the product small, calm, and honest about its limits. The whole point is a lightweight emotional support companion, not a general-purpose assistant.
