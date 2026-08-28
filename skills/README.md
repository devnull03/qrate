# Agent skills for qrate

Drop-in instructions that teach an AI coding agent to read the project open in a **running** qrate
and hand its findings back as drafts. They are packaged here so an archivist can download a folder
rather than being told to paste a protocol into a chat.

Every skill here is runtime-neutral. It says *when* to reach for qrate and *how to behave* while
doing so; the protocol itself — the endpoint, the token, every request and its limits — lives in
[`AGENTS.md`](../AGENTS.md) at the repo root, and each skill points there. One copy, so a method
that changes in the code cannot leave a second description of it standing.

**Download `AGENTS.md` alongside whichever skill you take.** Without it the skill is a signpost to
a file you do not have.

## What is here

| Skill | What it does |
|---|---|
| [`qrate-live-review`](qrate-live-review/SKILL.md) | Read the open project's columns, rows, diagnostics and selection over the local bridge; review them; stage findings back into qrate's Problems panel and Fixes menu as drafts. |

## Installing

Put the skill folder where your agent looks for skills, and put `AGENTS.md` where it reads project
instructions — usually the root of the folder you open the agent in.

- **Claude Code** — `~/.claude/skills/<skill>/` for every project, or `<project>/.claude/skills/`
  for one. It reads `AGENTS.md` from the working directory.
- **Pi** — `<pi-agent-dir>/skills/<skill>/`. qrate's *bundled* Pi runs with skills disabled and its
  own system prompt, so this is for a separate Pi installation, not the Agent panel.
- **Anything else** — the skill is plain Markdown. Load it and `AGENTS.md` as instructions.

## The one thing every agent must do

Send `X-Agent: <your runtime>` on every bridge call, naming yourself honestly. It is a label qrate
cannot verify, and it is all the archivist has to tell which agent did what in the Agent panel.

## What an agent can never do here

The bridge cannot change a cell. Staged findings are proposals that sit in the Problems panel until
the archivist clicks one. An agent that reports it corrected something is reporting a thing that
did not happen.
