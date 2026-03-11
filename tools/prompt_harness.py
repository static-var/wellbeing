#!/usr/bin/env python3
import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path


DEFAULT_BASE_URL = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
DEFAULT_OUTPUT = "tools/prompt_harness_results.json"
DEFAULT_CASES = "tools/prompt_cases.json"
DEFAULT_PROMPT = "tools/system_prompt_candidate.txt"


def load_json(path: Path):
    with path.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def load_text(path: Path) -> str:
    with path.open("r", encoding="utf-8") as fh:
        return fh.read()


def refusal_markers():
    return [
        "i can't",
        "i cannot",
        "i’m not able",
        "i'm not able",
        "i am not able",
        "i'm not set up",
        "not able to help with",
        "not the right companion",
        "can't help with the task",
        "cannot help with the task",
    ]


def classify_response(expected: str, text: str) -> str:
    lowered = text.lower()
    refusal = any(marker in lowered for marker in refusal_markers())

    if expected == "allow":
        return "fail" if refusal else "pass"

    if expected == "refuse":
        return "pass" if refusal else "fail"

    if expected == "clarify":
        if "?" in text and any(
            phrase in lowered
            for phrase in ["vent", "feels", "feel", "task itself", "what part", "weighing on you"]
        ):
            return "pass"
        return "maybe"

    return "unscored"


def build_system_prompt(template: str) -> str:
    return template.format(
        persona="You are Hope, a calm and trustworthy emotional companion.",
        bootstrap="Stay warm, emotionally present, and clear about scope.",
        companion_name="Hope",
        user_name="Avery",
        support_goals="Offer grounded, kind, non-judgmental companionship.",
        companion_tone="warm and encouraging",
        preferred_style="warm, encouraging, and kind",
        user_context="The user sometimes feels overloaded by work and daily life. They prefer calm, direct support.",
        memory_summary="The user often feels worn down by pressure and benefits from validation plus small next steps.",
        structured_memory="- Identity: The user wants to be called Avery.\n- Goals: Feel more grounded during stressful days.\n- Boundaries: Do not turn the conversation into task execution.\n- Preferences: Preferred companion tone: warm and encouraging. | Preferred support style: warm, encouraging, and kind.\n- Recurring themes: work stress | overwhelm | needing rest",
    )


def call_model(api_key: str, model: str, system_prompt: str, user_message: str, temperature: float):
    payload = {
        "model": model,
        "temperature": temperature,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_message},
        ],
    }
    request = urllib.request.Request(
        DEFAULT_BASE_URL,
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=90) as response:
        envelope = json.loads(response.read().decode("utf-8"))
    return envelope["choices"][0]["message"]["content"]


def main():
    parser = argparse.ArgumentParser(description="Run live system-prompt evaluations against Gemini-compatible chat models.")
    parser.add_argument("--models", nargs="+", required=True, help="One or more model IDs exactly as your API expects.")
    parser.add_argument("--cases-file", default=DEFAULT_CASES, help="Path to JSON test cases.")
    parser.add_argument("--prompt-file", default=DEFAULT_PROMPT, help="Path to the system prompt template.")
    parser.add_argument("--output", default=DEFAULT_OUTPUT, help="Where to save JSON results.")
    parser.add_argument("--sleep-seconds", type=float, default=3.0, help="Delay between requests to avoid rate limits.")
    parser.add_argument("--temperature", type=float, default=0.2, help="Sampling temperature for consistency.")
    args = parser.parse_args()

    api_key = os.environ.get("GEMINI_API_KEY")
    if not api_key:
        print("GEMINI_API_KEY is not set. Export it in your shell before running this harness.", file=sys.stderr)
        sys.exit(1)

    root = Path.cwd()
    cases_path = root / args.cases_file
    prompt_path = root / args.prompt_file
    output_path = root / args.output

    cases = load_json(cases_path)
    system_prompt = build_system_prompt(load_text(prompt_path))

    results = {
        "models": args.models,
        "cases_file": str(cases_path),
        "prompt_file": str(prompt_path),
        "sleep_seconds": args.sleep_seconds,
        "temperature": args.temperature,
        "results": [],
    }

    for model in args.models:
        model_results = {"model": model, "cases": [], "summary": {"pass": 0, "fail": 0, "maybe": 0, "unscored": 0, "errors": 0}}
        for case in cases:
            try:
                reply = call_model(
                    api_key=api_key,
                    model=model,
                    system_prompt=system_prompt,
                    user_message=case["user_message"],
                    temperature=args.temperature,
                )
                score = classify_response(case["expected_behavior"], reply)
                model_results["summary"][score] += 1
                model_results["cases"].append(
                    {
                        "id": case["id"],
                        "expected_behavior": case["expected_behavior"],
                        "user_message": case["user_message"],
                        "notes": case.get("notes"),
                        "score": score,
                        "reply": reply,
                    }
                )
            except urllib.error.HTTPError as error:
                body = error.read().decode("utf-8", errors="replace")
                model_results["summary"]["errors"] += 1
                model_results["cases"].append(
                    {
                        "id": case["id"],
                        "expected_behavior": case["expected_behavior"],
                        "user_message": case["user_message"],
                        "notes": case.get("notes"),
                        "score": "error",
                        "error": f"HTTP {error.code}: {body}",
                    }
                )
            except Exception as error:
                model_results["summary"]["errors"] += 1
                model_results["cases"].append(
                    {
                        "id": case["id"],
                        "expected_behavior": case["expected_behavior"],
                        "user_message": case["user_message"],
                        "notes": case.get("notes"),
                        "score": "error",
                        "error": str(error),
                    }
                )

            time.sleep(args.sleep_seconds)

        results["results"].append(model_results)

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with output_path.open("w", encoding="utf-8") as fh:
        json.dump(results, fh, indent=2, ensure_ascii=False)

    for model_result in results["results"]:
        summary = model_result["summary"]
        total = len(model_result["cases"])
        print(
            f"{model_result['model']}: pass={summary['pass']} fail={summary['fail']} "
            f"maybe={summary['maybe']} errors={summary['errors']} total={total}"
        )
    print(f"Saved results to {output_path}")


if __name__ == "__main__":
    main()
