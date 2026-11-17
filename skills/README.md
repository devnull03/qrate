# Agent skills for qrate

Drop-in instructions that teach an AI coding agent to reach a **running** qrate with the `qrate`
command, read the project open in it, and hand its findings back as drafts. They are packaged here
so an archivist can download a folder rather than being told to paste a protocol into a chat.

Every skill here is runtime-neutral. It says *when* to reach for qrate and *how to behave* while
doing so. The details live in exactly one place each, and every skill points there instead of
copying them:

- the options of every command are in the program: `qrate help <command>`;
- the contract of the `qrate agent` commands — every input, limit and exit code — is in
  [`AGENTS.md`](../AGENTS.md) at the repo root.

One copy, so a command that changes in the code cannot leave a second description of it standing.

**Download `AGENTS.md` alongside whichever skill you take.** Without it `qrate-live-review` is a
signpost to a file you do not have.

## What is here

| Skill | What it does |
|---|---|
| [`qrate-cli`](qrate-cli/SKILL.md) | Find the `qrate` command, open a project, check that qrate is running and which project is on screen, and read its summary. Start here. |
| [`qrate-live-review`](qrate-live-review/SKILL.md) | Read the open project's columns, rows, diagnostics and selection with `qrate agent`; review them; stage findings back into qrate's Problems panel and Fixes menu as drafts. |

The user-facing guide to both is [Agents and skills](../docs/cli/agents.md).

## Installing

Put the skill folders where your agent looks for skills, and put `AGENTS.md` where it reads project
instructions — usually the root of the folder you open the agent in.

- **Claude Code** — `~/.claude/skills/<skill>/` for every project, or `<project>/.claude/skills/`
  for one. It reads `AGENTS.md` from the working directory.
- **Pi** — `<pi-agent-dir>/skills/<skill>/`. qrate's *bundled* Pi runs with skills disabled and its
  own system prompt, so this is for a separate Pi installation, not the Agent panel.
- **Anything else** — the skills are plain Markdown. Load them and `AGENTS.md` as instructions.

## The one thing every agent must do

Pass `--agent <your runtime>` (or set `QRATE_AGENT`) on every `qrate agent` call, naming yourself
honestly. It is a label qrate cannot verify, and it is all the archivist has to tell which agent did
what in the Agent panel.

## What an agent can never do here

No `qrate` command can change a cell. Staged findings are proposals that sit in the Problems panel
until the archivist clicks one. An agent that reports it corrected something is reporting a thing
that did not happen.
