# Development

## Local setup

Run the app from the project root:

```bash
cargo run
```

Default local URL:

```text
http://127.0.0.1:8080/
```

## Useful commands

```bash
cargo check
cargo test
```

If you are validating live prompts against Gemini with your own local environment:

```bash
python3 tools/prompt_harness.py
```

## Repo layout

- `src/` — runtime, web handlers, guardrails, memory, gateways
- `static/` — landing page, onboarding, settings, chat, admin-adjacent UI
- `config/` — JSON runtime config
- `templates/tenant/` — `agent.md` and `bootstrap.md`
- `tools/` — prompt evaluation utilities
- `data/` — local SQLite state

## Memory development notes

The current memory system is intentionally lightweight:

- transcript history in SQLite
- rolling summaries
- structured memory buckets

When changing memory behavior, prefer realistic conversation-flow tests over tiny extraction-only tests. The main thing to protect is continuity without poisoning the prompt context.

## Safety development notes

Changes to prompts, scope rules, or memory should preserve the product boundary:

- emotional support is in scope
- work-task execution is out of scope
- medical/legal/clinical authority is out of scope
- crisis behavior must stay conservative

Do not position the product as therapy, diagnosis, or emergency response.

## Frontend notes

The default web experience lives in the static pages under `static/`. A/B-test variants should use separate routes/pages so the default experience remains stable while designs are compared.

Current experiment route:

- `/v2`
- `/v3`

## Manual verification checklist

- landing page loads
- signup/login works per tenant
- onboarding saves profile data
- chat persists history
- settings save correctly
- BYOK Gemini save/rotate flow works
- guardrails intercept out-of-scope prompts
- Telegram polling behaves sanely when configured

## Documentation rule

All public-facing docs should clearly state that Wellbeing is:

- an emotional support companion
- not a replacement for therapy or professional help
