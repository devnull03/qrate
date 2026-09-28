#!/usr/bin/env python3
"""Update the pinned version and release checksum in the qrate Homebrew Cask."""

import argparse
import re
from pathlib import Path


def replace_once(pattern: str, replacement: str, text: str, label: str) -> str:
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise SystemExit(f"expected one {label} stanza in the qrate Cask, found {count}")
    return updated


parser = argparse.ArgumentParser()
parser.add_argument("--cask", type=Path, required=True)
parser.add_argument("--version", required=True)
parser.add_argument("--sha256", required=True)
args = parser.parse_args()

if not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", args.version):
    raise SystemExit("Homebrew Cask updates require a stable three-part version")
if not re.fullmatch(r"[0-9a-fA-F]{64}", args.sha256):
    raise SystemExit("the DMG checksum must contain 64 hexadecimal characters")

text = args.cask.read_text(encoding="utf-8")
text = replace_once(r'^  version "[^"\r\n]+"$', f'  version "{args.version}"', text, "version")
text = replace_once(r'^  sha256 "[^"\r\n]+"$', f'  sha256 "{args.sha256.lower()}"', text, "sha256")
args.cask.write_text(text, encoding="utf-8", newline="")
