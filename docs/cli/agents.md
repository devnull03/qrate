# Agents and skills

An AI agent that you run on your own computer can read the project that is open in qrate.
It does this with the `qrate agent` commands. It can read rows, read the problems that qrate
found, and put its own findings in the Problems panel for you to review.

An agent cannot change a value that is already in a cell, and it cannot rename a file. It
sends those as drafts, and they change your data only when you apply them. An agent can
change the shape of the table, such as adding rows or columns, and each such change is one
step that you can undo. An agent cannot delete anything: it can only ask, and you decide.

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

An agent can also stage two kinds of bulk change. Each one is a single entry in the Problems
panel, however many rows it covers:

- **A column rewrite**, with `qrate agent stage-column-rewrite`: new values for many cells in
  one column.
- **File renames**, with `qrate agent stage-renames`: new names for files that rows link to.

Try the first two steps yourself:

```sh
qrate agent overview --agent me
echo '{"source":{"kind":"selected_rows"},"select":["Title"],"limit":10}' | qrate agent query --agent me
```

Every call appears in the **Log** tab of the Agent panel, with the name you gave after
`--agent`.

## Everything an agent can do

`qrate agent capabilities` lists everything an agent can ask qrate for, and the
[capability reference](capabilities.md) is the same list as a page. Besides the reads and drafts
above, an agent can stage changes across several columns at once (`stage_patch`), stage a whole
migration as one change set (`stage_changeset`), group variant spellings in a column
(`value_variants`), read History and notes, leave a signed note on a cell (`add_note`), find
where moved files went and relink them (`find_candidates`, `stage_relinks`), dry-run an export
(`export_check`), look closely at an image or find look-alikes (`image`, `similar`), look terms up
in authority files (`authority_search`), and ask you to keep one of its programs as a check
(`adopt_check`).

## A bulk edit from start to finish

You have a file, `mapping.csv`, with old and new identifiers, and you want every identifier
in the project changed and every scan renamed to match.

1. **Ask the agent.** For example: "Use mapping.csv to replace every Digital ID, and rename
   the photos to match."
2. **The agent reads.** It runs `qrate agent overview` for the columns and the `revision`. It
   reads the mapping file. To pair old values with rows it uses `qrate agent query`, or, for
   a large project, a small program with `qrate agent program-save` and
   `qrate agent program-run`. A program's answer is small, so the agent asks for the pairs a
   few hundred at a time.
3. **The agent stages.** It sends all the pairs in one `qrate agent stage-column-rewrite`
   call, and the file names in one `qrate agent stage-renames` call.
4. **You review.** Each changed cell is marked in the table: the old value is struck
   through in red and the new value is in green, on a cell with a yellow border. A bar above
   the table says how many changes the agent staged. Right-click a marked cell to apply or
   reject only that change.
5. **You apply.** Choose **Apply all** in the bar. The new values are one step and the file
   renames are another. qrate tells you how many it applied and how many it skipped because
   the cell or file changed after the agent read it.
6. **You can undo.** Press undo two times. The first undo puts the file names back on disk.
   The second puts the identifiers back.

## What an agent can change directly

An agent can add rows and columns, rename, move and describe a column, move, indent, outdent
and duplicate a row, filter the table, and leave a signed note on a cell. Each change is one step.
One undo reverses it, and the History panel shows who made it, for example "Add 3 rows ·
your name using pi".

## What an agent cannot do

- It cannot change a value that is already in a cell, except as a draft that you apply.
- It cannot delete a row or a column. It can ask, with a reason, and qrate shows you a dialog.
  Nothing is deleted unless you choose **Delete**.
- It cannot rename, move, or delete a file itself: renames and relinks are drafts you apply.
  It cannot apply its own drafts, and there is no setting that lets it.
- It cannot save the project.
- It cannot read files outside the project unless you allow each file or folder. For linked
  files it gets pictures that qrate makes and bounded text from documents.
- It cannot reach the network through qrate without your permission for each authority file.
- It cannot stage a finding against text that it did not read. Each finding carries the
  exact text of the cell that the agent judged. If the cell has changed since, qrate drops
  the finding.

A finding can propose a new value for a cell. The proposal is in that cell's **Problems**
menu. Nothing changes until you click it.

## The assistant's modes

The assistant in the Agent panel works in one of three modes, chosen per project with
`/qrate-mode` in the panel. A project starts in **review**.

| Mode | The assistant may |
|---|---|
| review | read the project and stage drafts for you to apply |
| edit | also change the table's shape, ask to delete, and read files you allow |
| connected | also look things up in authority files over the network, once you allow each one |

A step the mode does not allow is refused, even from inside a script the assistant writes. Type
`/qrate-follow` to let the assistant know what you select and do in qrate; it hears about it
with your next message and never starts working on its own.

On Linux and macOS, shell commands you approve for the assistant run in a sandbox that sees the
system, the project, a temporary folder and the files you allowed, read-only, with no network
outside connected mode. Windows has no such sandbox, so there your approval is the only guard.

## Turn it off

Open **Settings ▸ Agent** and switch off **Allow agents to read this app**. From then on,
every `qrate agent` command is refused. You do not have to restart qrate.

## For developers

The request and response types are in
[`crates/ai/src/agent.rs`](https://github.com/devnull03/qrate/blob/main/crates/ai/src/agent.rs).
[`AGENTS.md`](https://github.com/devnull03/qrate/blob/main/AGENTS.md) describes each command,
its limits, and its exit codes. A program that cannot run a command, such as a client that
only speaks MCP, is not supported yet.

## Files elsewhere on your machine

When you ask an agent to work from local files, it can request read-only access to the named file or folder. qrate shows you the path and the agent's reason. Choose **Allow read access** or **Deny**. A folder approval includes its descendants; approving one file does not include its siblings. Access lasts until qrate closes. Right-click the Agent panel's new-session button and choose **Revoke file access** to revoke all explicit grants, or ask the agent to forget its grants.

Pi checks these permissions each time it uses its built-in reading tools. Approving reads does not approve shell commands or writing files. The project folder and the project's configured files folder remain readable.

To make rows from approved files, the agent uses `import-files`, the same discovery, duplicate handling, and attachment mechanism qrate uses for file imports. It links originals where they are, skips files already represented, and records one History entry credited to the agent. One undo removes the import. Folder imports can preserve hierarchy when recursive discovery is requested.
