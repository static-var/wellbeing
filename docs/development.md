# Development

## Local setup

Run the app from the project root:

```bash
cargo run
```

Local URL:

```text
http://127.0.0.1:8080/
```

## Useful commands

```bash
cargo check
cargo test
python3 tools/prompt_harness.py
```

Use the prompt harness only when you want to test live Gemini behavior with your own key.

## Project layout

- `src/` - runtime, web handlers, memory, guardrails, gateways
- `static/` - landing, onboarding, chat, settings, and admin-facing pages
- `config/` - JSON runtime config
- `templates/tenant/` - `agent.md` and `bootstrap.md`
- `tools/` - prompt-testing utilities
- `data/` - local SQLite state

## Memory notes

The memory system is intentionally lightweight: transcript history, rolling summaries, and structured notes.

If you change memory behavior, prefer realistic conversation-flow tests. Tiny extraction-only tests miss the thing that matters most here: whether the companion still feels continuous from one session to the next.

## Safety notes

Keep the product boundary intact:

- emotional support is in scope
- work-task execution is out of scope
- medical, legal, and clinical authority are out of scope
- crisis behavior should stay conservative

Do not position Wellbeing as therapy, diagnosis, or emergency support.

## Manual smoke test

- landing page loads
- signup and login work per tenant
- onboarding saves profile data
- chat history persists
- settings save correctly
- Gemini BYOK save and rotate works
- guardrails catch out-of-scope requests
- Telegram polling behaves sanely when configured

## Docs rule

Public-facing docs should always say two things clearly:

- Wellbeing is an emotional support companion
- it is not a replacement for therapy or professional help
