#!/usr/bin/env python3

import json
import sys
import urllib.request
from pathlib import Path
from typing import List, Optional

SOURCE_URL = "https://models.dev/api.json"
PROVIDER_ID = "deepseek"
TIMEOUT_SECONDS = 30
USER_AGENT = "paloma-deepseek-models"

# Thinking mode is on by default with effort `high`:
# https://api-docs.deepseek.com/guides/thinking_mode
DEFAULT_EFFORT = "high"

OUTPUT = Path(__file__).resolve().parent.parent / "src" / "main" / "resources" / "models.json"


def log(message: str) -> None:
    print(message, file=sys.stderr)


def fail(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    sys.exit(1)


def fetch_catalogue() -> dict:
    request = urllib.request.Request(
        SOURCE_URL,
        headers={"User-Agent": USER_AGENT, "Accept": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=TIMEOUT_SECONDS) as response:
        return json.load(response)
    return None


def effort_values(reasoning_options: Optional[list]) -> List[str]:
    for option in reasoning_options or []:
        if option.get("type") == "effort":
            return list(option.get("values") or [])
    return []


def convert(model_id: str, model: dict) -> Optional[dict]:
    if model.get("status") == "deprecated":
        log(f"skip {model_id}: deprecated")
        return None

    reasoning_options = model.get("reasoning_options")
    efforts = effort_values(reasoning_options)
    if not efforts:
        # The plugin always sends `thinking: enabled` plus `reasoning_effort`,
        # so a model without effort levels cannot be driven by it.
        log(f"skip {model_id}: no reasoning effort levels")
        return None

    default_effort = DEFAULT_EFFORT if DEFAULT_EFFORT in efforts else efforts[0]
    if default_effort != DEFAULT_EFFORT:
        log(f"{model_id}: {DEFAULT_EFFORT!r} not supported, defaulting to {default_effort!r}")

    modalities = model.get("modalities") or {}
    return {
        "id": model_id,
        "name": model.get("name") or model_id,
        "input_modalities": modalities.get("input") or ["text"],
        "output_modalities": modalities.get("output") or ["text"],
        "reasoning": {
            "supported_efforts": efforts,
            "default_effort": default_effort,
        },
    }


def validate(entries: List[dict]) -> None:
    if not entries:
        fail("no models survived filtering")

    for entry in entries:
        model_id = entry["id"]
        if not isinstance(model_id, str) or not model_id:
            fail("model with empty id")
        if not isinstance(entry["name"], str) or not entry["name"]:
            fail(f"{model_id}: empty name")
        for field in ("input_modalities", "output_modalities"):
            value = entry[field]
            if not value or not all(isinstance(v, str) and v for v in value):
                fail(f"{model_id}: invalid {field} {value!r}")

        reasoning = entry["reasoning"]
        efforts = reasoning["supported_efforts"]
        if not efforts or not all(isinstance(v, str) and v for v in efforts):
            fail(f"{model_id}: invalid supported_efforts {efforts!r}")
        if reasoning["default_effort"] not in efforts:
            fail(f"{model_id}: default_effort {reasoning['default_effort']!r} not in {efforts!r}")


def main() -> None:
    try:
        catalogue = fetch_catalogue()
    except (OSError, ValueError) as error:
        fail(f"fetching {SOURCE_URL}: {error}")

    provider = catalogue.get(PROVIDER_ID) if isinstance(catalogue, dict) else None
    models = provider.get("models") if isinstance(provider, dict) else None
    if not isinstance(models, dict) or not models:
        fail(f"no {PROVIDER_ID!r} models in catalogue")

    entries = []
    for model_id, model in sorted(models.items()):
        if not isinstance(model, dict):
            fail(f"{model_id}: malformed entry")
        converted = convert(model_id, model)
        if converted is not None:
            entries.append(converted)
    validate(entries)

    text = json.dumps(entries, indent=2) + "\n"
    if OUTPUT.exists() and OUTPUT.read_text(encoding="utf-8") == text:
        log(f"{OUTPUT} unchanged ({len(entries)} models)")
        return

    OUTPUT.write_text(text, encoding="utf-8")
    log(f"wrote {len(entries)} models to {OUTPUT}")


if __name__ == "__main__":
    main()
