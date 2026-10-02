# Agents and skills

An AI agent that you run on your own computer can read the project that is open in qrate.
It does this with the `qrate agent` commands. It can read rows, read the problems that qrate
found, and put its own findings in the Problems panel for you to review.

An agent cannot change a cell. A finding from an agent is a draft. It changes your data only
when you click it.

## Before you start

1. Install qrate and check that the command works: `qrate version`. See
   [The qrate command](index.md).
2. Open your project in qrate.
3. Check that **Settings ▸ Agent ▸ Allow agents to read this app** is on. It is on by default.

## Skills

A skill is a short instruction file that teaches an agent how to do one job. qrate has two.
Each one is plain Markdown, so any agent can use it.

| Skill | What it teaches the agent | Where it is on GitHub |
|---|---|---|
| `qrate-cli` | To find the `qrate` command, open a project, and check which project is open | [skills/qrate-cli](https://github.com/devnull03/qrate/tree/main/skills/qrate-cli) |
| `qrate-live-review` | To read the open project, review it, and stage findings as drafts | [skills/qrate-live-review](https://github.com/devnull03/qrate/tree/main/skills/qrate-live-review) |

Both skills point to one more file, [`AGENTS.md`](https://github.com/devnull03/qrate/blob/main/AGENTS.md).
It holds the full contract of the `qrate agent` commands. Download it with the skills.

### Install a skill

Copy the skill's folder to the place where your agent looks for skills. Put `AGENTS.md` where
your agent reads project instructions. That is usually the folder you start the agent in.

| Agent | Where the skill folder goes |
|---|---|
| Claude Code | `~/.claude/skills/` for all projects, or `.claude/skills/` in one project |
| Codex and other agents that read `AGENTS.md` | Anywhere. Tell the agent to read the `SKILL.md` file. |
| Pi, installed by you | The `skills` folder of your Pi agent directory |

For example, for Claude Code:

```sh
git clone https://github.com/devnull03/qrate
mkdir -p ~/.claude/skills
cp -r qrate/skills/qrate-cli qrate/skills/qrate-live-review ~/.claude/skills/
cp qrate/AGENTS.md .
```

Then ask the agent in plain words, for example "review the rows I have selected in qrate".

## The assistant inside qrate

The **Agent** panel in qrate runs its own assistant, Pi. You do not install a skill for it.
qrate gives it the same `qrate agent` commands through the
[qrate Pi extension](https://github.com/devnull03/qrate-pi-extension), and starts it with the
right program and name. See [The Agent panel](../agent-panel.md).

## What an agent does

A review has the same steps for every agent.

1. **Overview.** The agent runs `qrate agent overview`. It learns the columns, how many rows
   there are, what you have selected, and how many problems qrate found. It also gets a
   `revision` number.
2. **Query.** The agent asks for the rows it needs with `qrate agent query`. qrate returns 20
   rows by default and 50 at most in one answer.
3. **Stage findings.** The agent sends its findings with `qrate agent stage-findings`. They
   appear in the Problems panel, beside the findings from qrate's own checks.

Try the first two steps yourself:

```sh
qrate agent overview --agent me
echo '{"source":{"kind":"selected_rows"},"select":["Title"],"limit":10}' | qrate agent query --agent me
```

Every call appears in the **Log** tab of the Agent panel, with the name you gave after
`--agent`.

## What an agent cannot do

- It cannot change, add, or delete a cell, a row, or a column.
- It cannot save the project.
- It cannot read your files directly. It gets small pictures that qrate makes, and bounded
  text from linked documents.
- It cannot stage a finding against text that it did not read. Each finding carries the
  exact text of the cell that the agent judged. If the cell has changed since, qrate drops
  the finding.

A finding can propose a new value for a cell. The proposal is in that cell's **Problems**
menu. Nothing changes until you click it.

## Turn it off

Open **Settings ▸ Agent** and switch off **Allow agents to read this app**. From then on,
every `qrate agent` command is refused. You do not have to restart qrate.

## For developers

The request and response types are in
[`crates/ai/src/agent.rs`](https://github.com/devnull03/qrate/blob/main/crates/ai/src/agent.rs).
[`AGENTS.md`](https://github.com/devnull03/qrate/blob/main/AGENTS.md) describes each command,
its limits, and its exit codes. A program that cannot run a command, such as a client that
only speaks MCP, is not supported yet.
