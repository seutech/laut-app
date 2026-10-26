#!/usr/bin/env python3
"""Check explicit Swift L("…") messages against the English catalog, offline."""
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent


def quoted_end(text, start):
    """Skip a quoted string inside an interpolation expression."""
    index = start + 1
    while index < len(text):
        if text[index] == "\\":
            index += 2
        elif text[index] == '"':
            return index + 1
        else:
            index += 1
    raise ValueError("Unterminated string")


def message(text, start):
    index = start + 1
    key = ""
    count = 0
    while index < len(text):
        if text.startswith("\\(", index):
            index += 2
            depth = 1
            while index < len(text) and depth:
                char = text[index]
                if char == '"':
                    index = quoted_end(text, index)
                    continue
                if char == "(":
                    depth += 1
                elif char == ")":
                    depth -= 1
                index += 1
            if depth:
                raise ValueError("Unterminated interpolation")
            key += "{" + str(count) + "}"
            count += 1
        elif text[index] == "\\":
            key += text[index:index + 2]
            index += 2
        elif text[index] == '"':
            return json.loads('"' + key + '"')
        else:
            key += text[index]
            index += 1
    raise ValueError("Unterminated localized message")


def main():
    catalog = json.loads((ROOT / "Sources/LautCore/Localization/en.json").read_text())
    problems = []
    count = 0
    for path in (ROOT / "Sources").rglob("*.swift"):
        text = path.read_text()
        for match in re.finditer(r'\bL\("', text):
            key = message(text, match.end() - 1)
            count += 1
            if key not in catalog:
                problems.append(f"{path.relative_to(ROOT)}: missing translation: {key!r}")
    for key, value in catalog.items():
        if not value or sorted(re.findall(r"\{\d+\}", key)) != sorted(re.findall(r"\{\d+\}", value)):
            problems.append(f"Invalid translation/placeholders: {key!r}")
    if problems:
        print("\n".join(problems), file=sys.stderr)
        return 1
    print(f"Localization catalog verified: {len(catalog)} translations, {count} Swift messages")
    return 0


if __name__ == "__main__":
    sys.exit(main())
