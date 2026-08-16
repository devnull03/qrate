---
title: AI tools
description: Connect an AI agent to these docs over MCP, or read them as plain markdown.
sidebar:
  order: 99
---

These docs are readable by agents as well as people. Nothing here needs an account or a key.

## MCP server

The endpoint at `https://qrate.dvnl.work/mcp` speaks
[Model Context Protocol](https://modelcontextprotocol.io) over stateless HTTP. It answers with
three tools: `search_docs`, `get_doc`, and `list_docs`.

Claude Code:

```sh
claude mcp add --transport http qrate-docs https://qrate.dvnl.work/mcp
```

Cursor, in `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "qrate-docs": { "url": "https://qrate.dvnl.work/mcp" }
  }
}
```

A client that only speaks stdio can bridge with
[`mcp-remote`](https://www.npmjs.com/package/mcp-remote):

```json
{
  "mcpServers": {
    "qrate-docs": {
      "command": "npx",
      "args": ["mcp-remote", "https://qrate.dvnl.work/mcp"]
    }
  }
}
```

The tool catalog is at [`/mcp-schema.json`](/mcp-schema.json) if you want to read what the
server offers before connecting.

## Plain markdown

Add `.md` to any docs URL for the source markdown, without navigation, styles, or scripts.
For example [`/docs/columns.md`](/docs/columns.md).

## llms.txt

- [`/llms.txt`](/llms.txt) — the page index
- [`/llms-full.txt`](/llms-full.txt) — every page in one file
- [`/llms-small.txt`](/llms-small.txt) — the same, trimmed for smaller context windows

## The Agent panel

Reading the docs is separate from reading a project. An agent running on your own machine can
read the project open in qrate through the [Agent panel](/docs/agent-panel), which is a
different mechanism with its own permissions.
