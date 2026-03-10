# Wellbeing

Wellbeing is a lightweight, self-hostable emotional support companion.

It is designed to feel like a calm, emotionally aware buddy across a small set of channels rather than a work copilot or general-purpose assistant. The current implementation focuses on a web portal and Telegram, with a runtime architecture that leaves room for WhatsApp and Discord later.

**Important:** Wellbeing is not a therapist, crisis service, or replacement for professional medical, mental-health, legal, or emergency support. It is a supportive companion for reflection, check-ins, and emotionally grounded conversation.

## What it does

- supports emotionally supportive chat in a lightweight Rust runtime
- keeps tenant/persona configuration in JSON plus per-tenant prompt files
- supports a web portal and Telegram today
- includes gentle scheduled check-ins
- supports encrypted personal Gemini BYOK
- stores conversations and lightweight memory in SQLite
- uses local guardrails before model calls
- supports prompt/persona files like `agent.md` and `bootstrap.md`

## Product boundary

Wellbeing is intentionally narrow:

- yes to emotional support, reflection, companionship, and gentle check-ins
- no to work-task completion, coding help, medical advice, legal advice, or crisis-professional positioning
- no claim that it replaces therapy, psychiatry, emergency care, or clinical judgment

If someone appears to be in crisis, the system should respond with safer boundary-aware guidance rather than pretending to be a clinician.

## Current architecture

- Rust runtime with Axum
- SQLite for auth, chat history, check-in state, and memory
- static web portal for landing, onboarding, chat, settings, and admin flows
- Telegram polling gateway
- Gemini OpenAI-compatible provider path
- optional shared Whisper worker for audio-note transcription

Key files:

- `config/config.json` — host/runtime config and tenant registry
- `templates/tenant/agent.md` — companion purpose and behavioral identity
- `templates/tenant/bootstrap.md` — startup behavior and memory expectations
- `src/guardrails.rs` — safety/scope rules and system prompt assembly
- `src/companion.rs` — chat flow, memory refresh, and provider selection
- `src/database.rs` — SQLite schema and persistence helpers

## Memory model

Wellbeing uses a lightweight structured memory system instead of a vector database.

Today it stores:

- recent transcripts in SQLite
- rolling session summaries
- structured memory items for:
  - identity
  - goals
  - boundaries
  - preferences
  - people
  - relationships
  - key events
  - recurring themes
  - session summary

Blocked and clarify-style turns are stored for audit, but they are intentionally excluded from provider context and should not refresh memory.

## Safety model

Safety is handled in layers:

1. local guardrails inspect the user message before any provider call
2. blocked or clarify-only turns are handled locally
3. only allowed turns are sent to the model
4. assistant output is sanitized before it is returned
5. unsafe prompt-like profile content is rejected before it enters the system prompt

This keeps the companion aligned with the product boundary: emotional support, not a job assistant or clinical authority.

## Quick start

### Requirements

- Rust toolchain
- a Gemini API key if you want live inference
- optional `WELLBEING_MASTER_KEY` if you want encrypted personal BYOK storage

### Run locally

```bash
cargo run
```

By default the app uses:

- config: `config/config.json`
- bind address: `127.0.0.1:8080`
- SQLite database: `data/wellbeing.sqlite`

Then open:

- `http://127.0.0.1:8080/`

## Configuration

Runtime configuration lives in `config/config.json`.

That file currently controls:

- bind address
- database path
- check-in scheduler settings
- Telegram gateway settings
- Whisper worker settings
- tenant definitions
- provider base URL and model
- gateway bindings

Each tenant points at:

- an `agent.md`
- a `bootstrap.md`
- a model/provider configuration
- gateway bindings
- a memory/storage path

## BYOK Gemini

Users can optionally provide their own Gemini API key in onboarding/settings.

- the key is encrypted at rest when `WELLBEING_MASTER_KEY` is configured
- the UI now asks for the key only
- the runtime chooses the Gemini model automatically

There is also a Gemini setup guide in the app:

- `/gemini-guide.html`

## Testing

Current test coverage includes:

- guardrail/scope regression tests
- profession-heavy false-positive and false-negative checks
- memory-flow tests that mock multi-turn conversations, session resets, structured memory capture, session summaries, and prompt-injection filtering
- people/relationship/event memory tests across session boundaries
- prompt hardening behavior
- scheduler and quiet-hours checks
- Whisper worker URL parsing tests
- runtime compile validation with `cargo check`

Recommended commands:

```bash
cargo check
cargo test
```

Latest local result:

- `46` tests passing

There is also a local prompt harness under `tools/` for real Gemini prompt evaluation:

- `tools/prompt_harness.py`
- `tools/prompt_cases.json`
- `tools/system_prompt_candidate.txt`

That harness is intended for local operator-run evaluation with your own API key.

## Deployment notes

Wellbeing is meant to stay lightweight:

- single Rust service
- SQLite-backed runtime state
- simple JSON configuration
- minimal operational surface

This makes it suitable for self-hosted deployments and for running multiple companion instances without building a heavy control plane first.

## Design experiments

The current default landing page is still served at:

- `/`

An alternate Gemini-generated A/B-test landing page is available at:

- `/v2`
- `/v2.html`

Use that variant to compare design direction without replacing the default experience.

A calmer, editorial V3 variant is also available at:

- `/v3`
- `/v3.html`

## Development docs

See:

- `docs/development.md`

## Open source intent

This project is meant to be understandable, hackable, and deployable by small teams or individual operators. If you contribute, please preserve the core product shape:

- lightweight
- emotionally supportive
- explicit about safety boundaries
- not a replacement for therapy or professional help
