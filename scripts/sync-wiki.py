"""Render docs into a cloned GitHub wiki, preserving pages not generated here."""

import argparse
import json
import posixpath
import re
from pathlib import Path
from urllib.parse import quote, unquote, urlsplit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("wiki", type=Path)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--ref", default="main")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    docs = sorted(
        source for source in (root / "docs").rglob("*.md")
        if source.relative_to(root / "docs").parts[0] != "dev"
    )
    pages = {}
    for source in docs:
        relative = source.relative_to(root / "docs")
        parts = list(relative.with_suffix("").parts)
        if parts[-1] == "index":
            parts.pop()
        name = "-".join(parts) or "Home"
        pages[source.relative_to(root).as_posix()] = name
    if len(set(name.lower() for name in pages.values())) != len(pages):
        raise ValueError("Docs contain conflicting wiki page names")

    repository = f"https://github.com/{args.repository}"
    ref = quote(args.ref, safe="")
    wiki_url = f"{repository}/wiki/"

    def rewrite(target, source, image=False):
        parsed = urlsplit(target)
        if parsed.scheme or parsed.netloc or not parsed.path:
            return target
        path = posixpath.normpath(
            posixpath.join(source.parent.relative_to(root).as_posix(), unquote(parsed.path))
        )
        suffix = (f"?{parsed.query}" if parsed.query else "") + (
            f"#{parsed.fragment}" if parsed.fragment else ""
        )
        if path in pages and not image:
            return wiki_url + quote(pages[path], safe="") + suffix
        kind = "raw" if image else "tree" if (root / path).is_dir() else "blob"
        return f"{repository}/{kind}/{ref}/{quote(path, safe='/')}" + suffix

    link = re.compile(r"(!?\[[^\n]*?\]\()([^\s)]+)([^\n)]*\))")
    reference = re.compile(r"^(\s{0,3}\[[^\]]+\]:\s*)(\S+)(.*)$")
    fence = re.compile(r"^\s{0,3}(`{3,}|~{3,})")
    generated = {}
    sections = {"User guide": [], "Getting started": [], "Plugins": []}
    for source in docs:
        content = source.read_text(encoding="utf-8")
        name = pages[source.relative_to(root).as_posix()]
        if name == "Home":
            content = re.sub(r"\n## For contributors\n.*?(?=\n## |\Z)", "", content, flags=re.S)
        lines = []
        fenced = None
        for line in content.splitlines(keepends=True):
            marker = fence.match(line)
            if marker:
                token = marker[1]
                if fenced is None:
                    fenced = token
                elif token[0] == fenced[0] and len(token) >= len(fenced):
                    fenced = None
                lines.append(line)
                continue
            if fenced is None:
                line = link.sub(
                    lambda match: match[1]
                    + rewrite(match[2], source, match[1].startswith("!"))
                    + match[3],
                    line,
                )
                line = reference.sub(
                    lambda match: match[1] + rewrite(match[2], source) + match[3], line
                )
            lines.append(line)
        source_url = f"{repository}/blob/{ref}/{source.relative_to(root).as_posix()}"
        generated[f"{name}.md"] = "".join(lines).rstrip() + (
            f"\n\n---\n\nGenerated from [the source documentation]({source_url}). "
            "Edit that file to update this page.\n"
        )
        title = next((line[2:].strip() for line in content.splitlines() if line.startswith("# ")), name)
        folder = source.relative_to(root / "docs").parts[0]
        section = {"getting-started": "Getting started", "plugins": "Plugins"}.get(
            folder, "User guide"
        )
        sections[section].append(f"- [{title}]({wiki_url}{quote(name, safe='')})\n")

    generated["_Sidebar.md"] = "\n".join(
        f"**{heading}**\n\n" + "".join(entries) for heading, entries in sections.items()
    )
    args.wiki.mkdir(parents=True, exist_ok=True)
    manifest = args.wiki / ".qrate-docs-pages.json"
    previous = json.loads(manifest.read_text(encoding="utf-8")) if manifest.exists() else []
    for name in previous:
        if Path(name).name != name or not name.endswith(".md"):
            raise ValueError(f"Invalid generated wiki filename: {name}")
        if name not in generated:
            (args.wiki / name).unlink(missing_ok=True)
    for name, content in generated.items():
        (args.wiki / name).write_text(content, encoding="utf-8", newline="\n")
    manifest.write_text(json.dumps(sorted(generated), indent=2) + "\n", encoding="utf-8")
    print(f"Rendered {len(docs)} documentation pages and the wiki sidebar")


if __name__ == "__main__":
    main()
