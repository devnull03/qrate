# Plugins, LSPs, and where qrate's validators fit

Research for **ASNT-45 — Column validation plugin system**. Written 2026-08-03.

This is two documents stapled together, and you can read them separately:

- **Part 1** explains how the Language Server Protocol actually works, mechanically, from scratch.
- **Part 2** surveys how real applications build plugin systems, and lands on what qrate should do.

The short version, if you only read one paragraph: **the two credible in-process diagnostic
systems — Neovim's `vim.diagnostic` and VS Code's `DiagnosticCollection` — independently arrived at
exactly the rule qrate already implements.** A producer publishes its complete set of problems, and
that replaces whatever it published before. Neither offers an "add one problem" call. Phase 1 of this
task shipped that rule without knowing it was the consensus design, which means phase 3 is a small
addition rather than a redesign.

---

# Part 1 — How LSP actually works

## 1. The problem it solves

Language intelligence — completion, go-to-definition, hover docs, rename, diagnostics — is expensive
to build and has to be built once per *language*. Editors each expose a different plugin API, so the
work historically had to be redone once per *(language, editor)* pair. M languages × N editors = M×N
integrations.

> "Implementing support for features like autocomplete, goto definition, or documentation on hover
> for a programming language is a significant effort. Traditionally this work must be repeated for
> each development tool, as each provides different APIs for implementing the same features."
> — [LSP overview](https://microsoft.github.io/language-server-protocol/overviews/lsp/overview/)

LSP turns M×N into M+N. Each language ships one *server*, each editor ships one *client*, and they
meet at a wire protocol.

**Why a protocol and not a library?** All three reasons are consequences of one decision — putting
the server in its own OS process:

1. **Implementation-language freedom.** `rust-analyzer` is Rust, `gopls` is Go, `clangd` is C++,
   `jdtls` is Java. A library would force everyone into the editor's runtime.
2. **Performance isolation.** VS Code's own guide: *"to correctly validate a file, Language Server
   needs to parse a large amount of files, build up Abstract Syntax Trees for them and perform static
   program analysis. Those operations could incur significant CPU and memory usage and we need to
   ensure that VS Code's performance remains unaffected."*
3. **Crash isolation.** A segfaulting compiler frontend takes down a subprocess, not the editor.

Hold onto these three. In Part 2 they are the exact test qrate fails, which is why qrate should not
spawn a server.

## 2. Transport and lifecycle

**JSON-RPC 2.0** over stdio (usually), pipes, or sockets. Messages are framed with an
HTTP-style header, because a stream of JSON objects has no self-delimiting boundary:

```
Content-Length: 96\r\n
\r\n
{"jsonrpc":"2.0","id":1,"method":"textDocument/hover","params":{...}}
```

`Content-Length` is mandatory. The blank line separates headers from the body. Read exactly that many
bytes, parse, repeat.

Three message shapes: **request** (has `id`, expects a response), **response** (has matching `id`,
plus `result` or `error`), and **notification** (no `id`, no reply, fire-and-forget). Diagnostics
under the push model are notifications, which is why the server can't know whether the client did
anything with them.

**The handshake** is strict and worth knowing:

1. Client → `initialize` (a request). Carries `ClientCapabilities`: everything this client can
   render or handle.
2. Server → response with `ServerCapabilities`: everything this server can do.
3. Client → `initialized` (a notification). Only now may either side send anything else.
4. …work…
5. Client → `shutdown` (request), then `exit` (notification).

**Capability negotiation is the versioning story.** There is no protocol version handshake beyond
this. A client announces `publishDiagnostics.tagSupport`, and a server that wants to send
`DiagnosticTag.Unnecessary` checks first. A server announces `codeActionProvider`, and a client that
doesn't see it never sends `textDocument/codeAction`. Features are added to LSP by adding optional
capability flags, and both sides degrade silently. That is the whole compatibility mechanism.

## 3. Document sync: the in-memory mirror

The server does **not** read your files off disk. The client owns the buffers and streams them over:

- `textDocument/didOpen` — here is the full text, version 1. The client now *owns* this document;
  the server must ignore whatever is on disk for it.
- `textDocument/didChange` — either the full new text (`TextDocumentSyncKind.Full`) or a list of
  range edits (`Incremental`). The server applies them to its mirror.
- `textDocument/didClose` — ownership returns to the filesystem.

Every document carries a monotonically increasing `version`. This matters more than it looks:
analysis is async, so a result computed against version 4 can arrive when the buffer is at version 7.
`PublishDiagnosticsParams.version` exists precisely so the client can drop stale results instead of
flashing wrong squiggles.

Worth internalising: **"open" means "the client owns this buffer", not "the user can see it".** A
client may have 200 documents open in the LSP sense with two tabs visible. The spec calls didOpen and
didClose *"ownership events"* and warns they *"don't necessarily reflect what the user sees in the
user interface"*. This is the exact confusion that forced pull diagnostics into existence.

## 4. Diagnostics — the part that matters here

Two models, both current as of 3.17.

### 4a. Push — `textDocument/publishDiagnostics`

A server → client notification. Everything about it flows from one sentence in the spec:

> **"Newly pushed diagnostics always replace previously pushed diagnostics. There is no merging that
> happens on the client side."**

and its consequence:

> "When a file changes it is the server's responsibility to re-compute diagnostics and push them to
> the client. **If the computed set is empty it has to push the empty array to clear former
> diagnostics.**"

Mechanically, the client keeps a map `(server, uri) → Diagnostic[]` and each notification does
`map[uri] = params.diagnostics`, wholesale. There is no per-diagnostic identity, no delete-by-id, no
partial update. Four consequences you must design around:

- **Invalidation is entirely the producer's job.** The spec says diagnostics are *"owned"* by the
  server, *"so it is the server's responsibility to clear them if necessary."*
- **To clear a file, publish `[]`.** Forgetting this is the classic bug: an error stays squiggled
  forever after the user fixes it, because the server just stopped mentioning the file.
- **You cannot report a subset.** If you re-run only your type-checker and publish its results, you
  have just erased your linter's results for that URI. Everything a server wants shown for a URI must
  be in every notification for that URI.
- **Files you stop analysing must be explicitly cleared.**

The `Diagnostic` structure:

```typescript
interface Diagnostic {
  range: Range;                       // required — where the squiggle goes
  severity?: DiagnosticSeverity;      // Error=1, Warning=2, Information=3, Hint=4
  code?: integer | string;            // machine-readable rule id: "no-unused-vars"
  codeDescription?: { href: URI };    // renders `code` as a link to the rule's docs
  source?: string;                    // human-readable producer: "eslint"
  message: string;                    // required
  tags?: DiagnosticTag[];             // Unnecessary=1 (fade out), Deprecated=2 (strikethrough)
  relatedInformation?: DiagnosticRelatedInformation[];   // secondary locations, possibly other files
  data?: LSPAny;                      // opaque, round-tripped back on codeAction
}
```

Three fields are more interesting than they look:

- **`severity`.** *"To avoid interpretation mismatches … it is highly recommended that servers always
  provide a severity value."* `Hint` is special in practice: VS Code renders it with **no squiggle at
  all** and excludes it from problem counts. It participates in quick fixes but never nags. qrate's
  `Severity::Note` already occupies exactly this slot — `Diagnostics::count` counts errors and
  warnings only, so a project full of imported notes doesn't light up a warning triangle.
- **`relatedInformation`.** Secondary locations, *"e.g. when duplicating a symbol in a scope"*. This
  is how you express "this cell is wrong *because of* that other cell" without inventing cross-file
  diagnostics. Diagnostics themselves are normatively *"only valid in the scope of a resource"*.
- **`data`.** *"A data entry field that is preserved between a `textDocument/publishDiagnostics`
  notification and a `textDocument/codeAction` request."* The client round-trips it untouched. This
  lets the server stash "how to fix this" without recomputing it. See §5.

A full example, and the clear that follows it:

```json
{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{
  "uri":"file:///proj/main.py","version":2,
  "diagnostics":[{
    "range":{"start":{"line":3,"character":4},"end":{"line":3,"character":8}},
    "severity":1,"code":"undefined-name","source":"mylint",
    "message":"Undefined name 'prnt'. Did you mean 'print'?",
    "data":{"fixKind":"rename-symbol","candidate":"print"}}]}}
```

```json
{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{
  "uri":"file:///proj/main.py","version":3,"diagnostics":[]}}
```

### 4b. Pull — `textDocument/diagnostic` (added in 3.17)

The spec's own rationale, verbatim:

> "This model has the advantage that for workspace wide diagnostics the server has the freedom to
> compute them at a server preferred point in time. On the other hand the approach has the
> disadvantage that **the server can't prioritize the computation for the file in which the user
> types or which are visible in the editor**. Inferring the client's UI state from the
> `textDocument/didOpen` and `textDocument/didChange` notifications might lead to false positives
> since **these notifications are ownership transfer notifications**."

The core problem with push: **the server has no idea what the user is looking at.** Under pull, the
*client* — which knows its viewport, focus, and idle state — asks for exactly the documents it wants,
when it wants them. Because a pull is a request rather than a notification, it can also be cancelled
(`$/cancelRequest`), report progress, stream partial results, and carry a cache token.

That cache token is `previousResultId`. The server replies with either a **full** report (here is the
complete set, and here is a new result id) or an **unchanged** report (*"the last returned report is
still accurate"*), which costs one round trip and no recomputation.

**Which to use.** Push is right when the producer knows when its own results change and the set of
things it watches is small. Pull is right when computing is expensive, the set is large, and only the
producer's *consumer* knows what's worth computing. For qrate: a sheet is one "document" and the
whole thing is in memory, so push is the obvious fit — and it is what's already built.

## 5. Code actions — why a fix is data, not a callback

```typescript
// Request
{"method":"textDocument/codeAction","params":{
  "textDocument":{"uri":"..."},"range":{...},
  "context":{"diagnostics":[ /* the diagnostics at this range */ ],"only":["quickfix"]}}}
```

Notice `context.diagnostics`: **the client hands the server back the diagnostics the server itself
published.** The fix provider does not re-analyse; it filters the diagnostics it's given, usually by
`code`, and emits fixes. That push/pull split — push the problems, pull the fixes — is the single
cheapest thing to copy, and it is what makes the lightbulb fast.

A `CodeAction` carries a `WorkspaceEdit` — a *description* of a text change — not a callback:

```typescript
interface CodeAction {
  title: string;
  kind?: CodeActionKind;      // "quickfix", "refactor.extract.function", "source.organizeImports"
  diagnostics?: Diagnostic[]; // which problems this action resolves
  edit?: WorkspaceEdit;       // data
  command?: Command;          // escape hatch: an imperative call back into the server
  isPreferred?: boolean;
}
```

Describing the fix as data rather than a callback buys four things at once: it is serializable across
the process boundary; it can be *previewed* before applying; it participates in undo as a normal
edit; and it is testable without any UI. The `command` escape hatch exists for fixes that need to ask
the user something or touch state outside the document.

`kind` is a dotted hierarchy with prefix matching, so a client asking for `refactor` gets
`refactor.extract.function` too.

**Lazy resolution.** `provideCodeActions` can return cheap actions with only `title`/`kind`/
`diagnostics`, and the expensive `edit` gets computed in `codeAction/resolve` when the user actually
selects the item. The rule is blunt: *"A code action that has an edit will not be resolved."* Set the
edit eagerly and you opt out of laziness.

## 6. Positions, and the UTF-16 landmine

```typescript
interface Position { line: uinteger; character: uinteger; }
```

Both 0-based. And then:

> `character` is *"the offset in the line **in UTF-16 code units**"*.

This is LSP's most-criticised decision, and it is a fossil of being born inside VS Code, whose
strings are UTF-16. It means a Rust server holding UTF-8 buffers must convert offsets on **every**
position in **every** message, in both directions — and that an emoji outside the BMP counts as 2.

3.17 added `positionEncoding` negotiation (`utf-8` / `utf-16` / `utf-32`), but `utf-16` remains the
mandatory default, so a server still has to implement it.

**For qrate this maps to nothing and should be dropped entirely.** There is no line/character
coordinate — a location is `(row, column-name)` and, if sub-cell spans ever arrive, a **byte** range.
gpui's `StyledText::with_highlights` takes byte `Range<usize>` natively and `debug_assert!`s
`is_char_boundary`. Copying LSP's `Position` here would import a bug, not a design.

## 7. Everything else, briefly

`textDocument/completion` (+ `completionItem/resolve`, the same laziness pattern), `semanticTokens`
(with delta requests), `workspace/symbol`, `workspace/didChangeConfiguration`, `$/progress` with
work-done tokens, and `$/cancelRequest`.

## 8. What LSP gets criticised for

- **UTF-16 positions.** See above. Universally disliked.
- **Edge-triggered, not level-triggered.** Everything is "here's what changed", so any dropped or
  reordered message desynchronises the mirror permanently. There is no "resend me the truth" request.
- **The sync burden.** Every server reimplements incremental text application, and it is a rich
  source of off-by-one bugs.
- **Latency and statefulness.** A round trip per interaction, over a stream that must stay in order,
  with a stateful mirror on both ends.
- **Over-generality.** The protocol is large; most servers implement a fraction; clients must handle
  every combination of absent capabilities.

Notice how many of these are costs of *the process boundary*, not of the diagnostic model. The
diagnostic model itself — publish a complete set, replace by owner — survives the critique intact.
That is why it is worth copying while the transport is not.

## 9. LSP vs. plugin systems

They are orthogonal, and conflating them is the most common confusion in this space.

An **extension** is glue that runs inside the editor. A **language server** is a separate program
that computes. In VS Code, Zed, and Neovim alike, the "language extension" for a given language is
usually a few dozen lines whose entire job is deciding which binary to launch, with what arguments.
The extension does not analyse anything.

Which sets up Part 2.

---

# Part 2 — How real applications build plugin systems

## 1. Neovim — the closest prior art

Neovim is worth studying above the others because its diagnostic framework is a general-purpose
problem list that LSP happens to be *one* producer for. That is exactly qrate's shape.

### Namespaces: the replace rule

A producer creates a **named** namespace, then publishes into it:

```lua
local ns = vim.api.nvim_create_namespace("my-linter")   -- anonymous namespaces WILL NOT WORK
vim.diagnostic.set(ns, bufnr, diagnostics)
```

The storage is literally `bufnr → ns → Diagnostic[]`, and `set` is one table assignment:

```lua
if vim.tbl_isempty(diagnostics) then
  diagnostic_cache[bufnr][namespace] = nil
else
  diagnostic_cache[bufnr][namespace] = diagnostics
end
```

**No merge, no diffing, no per-diagnostic identity, no "update one" API.** Passing an empty list is
the idiomatic clear. The public wrapper is three steps — *store → render → announce*:

```lua
function M.set(namespace, buf, diagnostics, opts)
  M._store.set(namespace, buf, diagnostics)
  M.show(namespace, buf, nil, opts)
  api.nvim_exec_autocmds('DiagnosticChanged', { buf = ..., data = { diagnostics = diagnostics } })
end
```

The docs state the producer/consumer split explicitly: *"The APIs for producers require a
{namespace} as their first argument, while those for consumers generally do not."*

Neovim keeps **three orthogonal states** where qrate currently has one, and the distinction is worth
knowing even if we never need it:

| | data removed? | can `show()` bring it back? |
|---|---|---|
| `set(ns, buf, {})` | yes | no |
| `reset(ns, buf)` | yes | no |
| `hide(ns, buf)` | no | yes |
| `enable(false, …)` | no | not until re-enabled |

### Handlers: the best idea in the survey

This is the mechanism I'd most want to steal *eventually*, and the reason to write it down now.

Neovim separates **who produces diagnostics** from **how they are displayed**. Display is a plain
table of handlers, each with `show` and optionally `hide`:

```lua
--- @class vim.diagnostic.Handler
--- @field show? fun(namespace, bufnr, diagnostics, opts)
--- @field hide? fun(namespace, bufnr)
```

Registration is *literally a table assignment*:

```lua
vim.diagnostic.handlers["my/notify"] = {
  show = function(namespace, bufnr, diagnostics, opts)
    vim.notify(("%d diagnostics in buffer %d"):format(#diagnostics, bufnr), opts["my/notify"].log_level)
  end,
}
```

And **the built-ins use the same public path — there is no privileged registration**:

```lua
M.handlers.signs        = { show = ..., hide = ... }
M.handlers.underline    = { show = ..., hide = ... }
M.handlers.virtual_text = { show = ..., hide = ... }
M.handlers.virtual_lines= { show = ..., hide = ... }
```

The entire dispatcher is eight lines, and the core never enumerates display kinds:

```lua
for handler_name, handler in pairs(vim.diagnostic.handlers) do
  if handler.show and opts_res[handler_name] then
    local filtered = filter_by_severity(opts_res[handler_name].severity, diagnostics)
    handler.show(namespace, bufnr, filtered, opts_res)
  end
end
```

Note `pairs()` — **handler order is unspecified**, so handlers must be independent.

Maintainer rationale, from the PR that introduced it
([neovim#16137](https://github.com/neovim/neovim/pull/16137)):

> "Rather than treating `virtual_text`, signs, and underline specially, introduce the concept of
> generic 'handlers', of which those three are simply the defaults bundled with Nvim."

**And it demonstrably paid off.** `virtual_lines` shipped in core in 0.11 by porting
`lsp_lines.nvim` — a plugin that had existed in userland for *years* as nothing but a handler,
requiring zero core support. That is the proof that the extension point was in the right place.

A handler need not be a decoration at all. The documented loclist example is a handler with no
`hide` that repopulates a list on every change — which is to say, **the Problems panel is a
handler**:

```lua
vim.diagnostic.handlers.loclist = {
  show = function(_, _, _, opts) vim.diagnostic.setloclist(opts.loclist) end
}
```

### nvim-lint: the direct analogue of qrate's validators

[nvim-lint](https://github.com/mfussenegger/nvim-lint) is a pure-Lua, non-LSP linter runner. Its
README states the scope precisely: *"It spawns linters, parses their output, and reports the results
via the `vim.diagnostic` module."* The whole integration is four things:

1. **One namespace per linter name**, auto-created on first use.
2. **A registry that is pure data**: `M.linters_by_ft = { markdown = {'vale'}, python = {'ruff','mypy'} }`.
3. **A parser contract** — the linter author's only job is producing the framework's own diagnostic type.
4. **Publish is one `set` call**, guarded by a liveness check.

**nvim-lint contains no display code whatsoever.** Because the namespace is per linter, a user can
style one linter differently; because `set` replaces, a linter that now returns zero problems clears
exactly its own results and nothing else.

This is, almost line for line, the architecture phase 3 just built.

### Extmarks, and a warning worth heeding

Neovim anchors decorations to **extmarks** — marks that live in the buffer and move with edits —
rather than to line numbers. The properties that matter: `right_gravity` (which way the mark shifts
on insertion), `invalidate` (hide the mark if its whole range is deleted, and self-report as dead),
`undo_restore`, `priority`, and namespaced bulk clear (`nvim_buf_clear_namespace`) so one owner's
decorations can be wiped without touching anyone else's.

**The warning.** Neovim only started positioning diagnostics via extmarks recently
([#34014](https://github.com/neovim/neovim/pull/34014)), and it is not yet consistent. From the open
issue [#35136](https://github.com/neovim/neovim/issues/35136):

> "The output of `vim.diagnostic.get()` can return the outdated data about positions. In particular,
> it returns the original data. This can become even more confusing when combined with the output of
> `get_next()` / `get_prev()` which use data from extmarks."

So `get()` returns stored positions and `jump()` returns extmark-derived ones: **two sources of truth
for the same field in the same public API.** This is the strongest possible argument for the
`ponytail:` comment already sitting on `Location.row` — that a positional row index becomes
`dataset_main._row_id` once rows can be reordered. If qrate ever gets row reordering, decide the
single source of truth for a diagnostic's position *before* shipping it, not after.

### Re-running is the producer's problem

Neovim provides **no scheduler**. The core does not know what a linter is or when it should run;
nvim-lint's README just tells you to write the autocmd yourself. Consequently `vim.diagnostic` has no
notion of "stale" or "in progress" — a deliberate decision, not an omission.

nvim-lint uses **cancellation, not debouncing**: starting a run kills the in-flight process for the
same linter, and a `cancelled` flag is re-checked in `publish` so a late-finishing old process cannot
clobber newer results. Neovim's only built-in debounce is insert-mode deferral (`update_in_insert`,
default off), and it coalesces by overwrite — `bufs_waiting_to_update[bufnr][namespace] = args` —
keeping only the latest pending render rather than a queue. That is the right shape for a debounce.

### In-process plugin loading, briefly

Plugins are Lua files on `runtimepath`, sourced in three phases: `plugin/` → packages →
`after/plugin/`. `after/` is the documented override hook. Modules under `lua/` load on demand via
`require`, resolved against `runtimepath` in order — **first wins, with no versioning and no conflict
detection** — and cached permanently after first load. Autocommands are the hook bus: a plugin
subscribes to host lifecycle events without the host knowing it exists.

The versioning/deprecation story is real but informal. `goto_next`/`goto_prev` collapsed into
`jump({count = ±1})`, mirroring Vim's own count semantics and enabling `]D`/`[D` for free; the old
names still work and are listed in `deprecated.txt` with their replacements.

## 2. VS Code — the second data point

The same shape, reached independently.

```typescript
const collection = vscode.languages.createDiagnosticCollection('qrate');
collection.set(uri, diagnostics);   // "Will replace existing diagnostics for that resource."
collection.delete(uri);             // === set(uri, undefined)
collection.clear();
```

**Same wholesale replace, and again there is no "add one diagnostic" API.**

The collection's `name` is the **owner key** — markers are stored per `(owner, resource)`, so two
collections never disturb each other. It is *not* what gets rendered: that's `Diagnostic.source`. Two
separate concepts that are easy to conflate.

**Diagnostics without a language server is a first-class, documented path.** The official
`code-actions-sample` gives the canonical lifecycle in one function:

```typescript
export function refreshDiagnostics(doc, collection) {
  const diagnostics = [];
  for (let i = 0; i < doc.lineCount; i++) { /* … */ }
  collection.set(doc.uri, diagnostics);      // full replace
}

vscode.workspace.onDidChangeTextDocument(e => refreshDiagnostics(e.document, collection));
vscode.workspace.onDidCloseTextDocument(doc => collection.delete(doc.uri));
```

Four rules encoded there, all of which transfer:

1. **Seed on activation** — events alone under-report, because activation can happen after the
   document is already open.
2. **Recompute and replace**, never incremental patching.
3. **Deletion is explicit.** VS Code does *not* clear diagnostics when a document closes. This is the
   most commonly missed step and the most common source of stale markers.
4. **There is no built-in debouncing**, and no official guidance on it. The sample recomputes on
   every keystroke; real extensions hand-roll a 300–500 ms timer. You own the scheduling.

**Squiggles are automatic and non-negotiable.** You publish a range and a severity; the editor draws
the underline, the ruler tick, the minimap mark, the hover, and the Problems row. Colour comes from
the theme (`editorError.foreground`, etc.). An extension cannot restyle its own squiggles — for
bespoke visuals there's a completely separate mechanism, `createTextEditorDecorationType`, which
feeds none of the Problems panel, quick-fix, or F8-navigation machinery. The idiomatic answer is
both: diagnostics for semantics and navigation, decorations layered on top for looks.

`DiagnosticTag` is worth noting as a design idea: `Unnecessary` renders faded, `Deprecated` renders
struck through. That's **a rendering channel kept separate from severity**, so you don't end up
overloading severity to mean presentation.

### Problem matchers — a zero-code validator plugin

The genuinely interesting third path. A problem matcher regex-scrapes a task's stdout into the
Problems panel with **no extension code at all** — pure JSON in a manifest:

```json
{ "name": "gcc", "owner": "cpp", "fileLocation": ["relative", "${workspaceFolder}"],
  "pattern": { "regexp": "^(.*):(\\d+):(\\d+):\\s+(warning|error):\\s+(.*)$",
               "file": 1, "line": 2, "column": 3, "severity": 4, "message": 5 } }
```

The fields are **1-based capture-group indices**, not values. `owner` is the same namespace as
`DiagnosticCollection.name`, which is how a task's scraped problems can be merged with, or kept
separate from, a programmatic producer's. Background/watch tasks get `beginsPattern` and `endsPattern`
— which is the replace-semantics of `set` expressed declaratively for a streaming producer.

**If qrate ever wants third-party validators without a code boundary at all, this is the design.** A
manifest declaring a regex, capture positions, an owner, and a closed severity vocabulary. Note that
the severity is a *captured string mapped through a fixed vocabulary*, never an arbitrary value —
that's what keeps the plugin surface closed.

### When VS Code says to use a language server

Its guide gives exactly three reasons, quoted in Part 1 §1: different runtime, expensive analysis,
M×N reuse. Read honestly against qrate:

1. **Different runtime** — void. Our validators are Rust, in the same process as the app. This reason
   exists *because* VS Code is Node and language tooling usually isn't.
2. **Expensive analysis** — the only one with residual force, and it argues for getting work off the
   UI thread, not for a socket. A worker thread with a cancellation token buys the same isolation at
   roughly zero marshalling cost.
3. **M×N reuse** — void. N = 1. It's our app.

**VS Code's own guidance, applied to qrate, endorses not spawning a server.**

## 3. The out-of-process family, and why it loses

### Zed — and the finding that settles the argument

Zed's extensions are WebAssembly components run by embedded wasmtime, with a genuinely good
versioning story: the API version is stamped into the wasm binary as a custom section named
`zed:api-version` (three big-endian `u16`s), and old WIT worlds are kept in-tree forever under
`since_v0.0.1` … `since_v0.8.0`, so old extensions keep running. There's a real capability system
too, declared in `extension.toml` (`process:exec`, `download_file`, `npm:install`), and epoch
interruption every 100 ms so a runaway extension is preemptible.

**And none of it can produce a diagnostic.** The complete export list of the current WIT world is:

```
init-extension, language-server-command, language-server-initialization-options,
language-server-workspace-configuration, labels-for-completions, labels-for-symbols,
complete-slash-command-argument, run-slash-command, context-server-command,
suggest-docs-packages, index-docs, get-dap-binary, dap-request-kind, …
```

There is no `diagnostic` record anywhere in the WIT — `lsp.wit` defines only `completion`, `symbol`,
and their label types. A Zed extension returns a `record command { command, args, env }` and Zed
spawns that **out-of-process binary**, whose `publishDiagnostics` Zed consumes natively.

**A Zed wasm extension is an argv/config factory and a label prettifier, not a computation
participant.** Zed says as much themselves:

> "our extension API only allows users to add new language support, themes, snippets, and slash
> commands. There's no support for modifying the UI to create new panels, or making arbitrary HTTP
> requests, or touching the file system" — [Zed Decoded: Extensions](https://zed.dev/blog/zed-decoded-extensions)

So "do what Zed does" for column validation means: **ship `qrate-validate.exe` and JSON-RPC every
cell to it.** That is the whole proposal, stated plainly, and it is obviously wrong for us.

Worth noting: most Zed extensions ship *zero wasm*. Themes, languages, grammars, snippets, and
language-server declarations are all manifest entries needing no code.

### The rest, briefly

- **Lapce** — WASI plugins, but in practice *"communication with an LSP implementation is the only
  feature available through Lapce plugins."* Same argv-factory shape as Zed.
- **Helix** — the most useful case study. Shipped for years with **no plugin system at all**.
  `archseer`: *"we had three separate attempts at integrating WASM that didn't lead anywhere."*
  `pascalkuthe` noted plugins need access to the Rust ecosystem for perf-sensitive work that wasm
  can't reach. The chosen direction — an embedded Steel/Scheme VM — is still **unmerged** and labelled
  `S-experimental`. A widely-used editor, years in, still has no plugin boundary, and it was not
  fatal.
- **rust-analyzer** — no extension mechanism whatsoever. A flat workspace of crates with explicitly
  *non-API* internal boundaries; the only boundary is LSP at the outer edge. Diagnostics are just a
  crate (`ide_diagnostics`).
- **Rust `dylib` / `abi_stable`** — don't. `repr(Rust)` layout is explicitly unstable across compiler
  versions and optimization levels, with nothing in the linker able to detect a mismatch. The classic
  failure is the host reading a `Vec`'s first 8 bytes as the pointer while the plugin wrote the
  length. It forces `#[repr(C)]` on everything crossing the line, per-OS/arch binaries, and a
  **recompile of every plugin on every rustc bump**. Essentially no shipping Rust editor uses it for
  third-party plugins.
- **Extism** — the cheapest wasm on-ramp (PDKs for 8+ languages, hides the linear-memory
  marshalling), but its contract is effectively `bytes -> bytes`, so you own the encoding and its
  evolution.

### When a boundary isn't worth it

The best single source is Maël Nison (Yarn), *[Plugin systems: when & why?](https://dev.to/arcanis/plugin-systems-when-why-58pp)*.
His case *for* is organisational, not technical — plugins are boundaries that stop a long-lived,
high-turnover OSS codebase from rotting into "no one dares change anything." His case against is one
sentence:

> **"plugins can only be designed once you already have a perfect knowledge of the design space."**

He waited about two years before adding one to Yarn: *"it took almost two years before I finally felt
confident enough… Before that, I spent my time writing various package manager implementations."*

## 4. Comparison

| System | Boundary | Isolation | Can a plugin compute a diagnostic? | Versioning story |
|---|---|---|---|---|
| **Zed** | wasm component, in-proc | memory + hang (epoch) + manifest capabilities | **No.** No diagnostic type in WIT; returns an argv for an out-of-process LSP | Best in class: `zed:api-version` + `since_v*` worlds kept forever |
| **VS Code** | separate Node process | crash + hang; weak authority | **Yes** (`createDiagnosticCollection`), though the docs steer you to a server | `engines.vscode` semver + additive optional API |
| **Neovim (Lua)** | embedded VM, in-proc | none | Yes, `vim.diagnostic` | informal; deprecations documented with replacements |
| **Neovim (remote)** | separate process, msgpack-RPC | crash; hang only for async | Yes, same API over RPC | generated manifest, lazy host spawn |
| **Helix** | *none shipped* | — | would be, but unmerged | n/a |
| **Lapce** | WASI module | memory | **No** in practice | registry + manifest |
| **rust-analyzer** | none (in-tree crates) | n/a | Yes — it's just a crate | n/a |
| **Rust dylib** | native cdylib | none | Yes | **Worst.** Recompile everything on every rustc bump |
| **qrate (now)** | in-process Rust trait, crate per validator | none | **Yes — that's the point** | Cargo semver on the `diagnostics` crate |

---

# Part 3 — What qrate does, and why

## The verdict

**No wasm. No subprocess. No JSON-RPC.** A `dyn ColumnValidator` trait with one crate per validator.

The `dyn` is not justified by "someone might add a validator someday" — that's the speculative
argument the codebase audit deletes. It is justified structurally and in the present tense:
**spell-check needs a ~2.2 MB dictionary crate, and if validators were a closed enum inside the crate
`table` depends on, that dictionary would be linked into `table → workspace → app`.** The trait, plus
registration at the composition root, is what keeps it out. Closed enums stay closed everywhere
inside (`Severity`, `Source`).

## The convergence

| | producer scope | publish call | replace rule |
|---|---|---|---|
| LSP | server + URI | `textDocument/publishDiagnostics` | all for `(server, uri)` |
| Neovim | named namespace | `vim.diagnostic.set(ns, bufnr, items)` | all for `(ns, buffer)` |
| VS Code | collection `name` (owner) | `collection.set(uri, items)` | all for `(owner, uri)` |
| **qrate** | `Source::Validator(name)` | `Diagnostics::set(source, dataset, items, cx)` | all for `(source, dataset)` |

Three independent systems, one rule. qrate's was already correct.

## The v1 API

Trait and registry live in `crates/diagnostics`; `table` already depends on it, so this adds no
edges. Each validator is its own crate.

```rust
pub struct ColumnInfo<'a> {
    pub name: &'a str,               // header text; also the name diagnostics are filed under
    pub data_type: &'a str,          // from the project's __columns
    pub settings: &'a ColumnSettings, // where a validator's own knobs live
}

pub trait ColumnValidator: 'static {
    fn name(&self) -> SharedString;
    fn validate(&self, column: &ColumnInfo, values: &[SharedString])
        -> Vec<(usize, Severity, SharedString)>;
}
```

**A validator never builds a `Location` or a `Source`.** It reports `(row, severity, message)` and
the registry addresses it — the same split as an LSP server reporting ranges while the client owns
the URI, and the same split as nvim-lint's parser contract. That is precisely what lets a validator
live in its own crate knowing nothing about datasets, projects, or the table.

Returning an empty vec is how a validator opts out of a column it doesn't apply to. There is
deliberately no `applies_to` method: one method is one thing for a third party to get wrong instead
of two.

`Validators::run` issues one `Diagnostics::set` per validator carrying every column it flagged, so a
re-run is self-invalidating — a fixed cell disappears because the next run simply doesn't report it.
There is no clear API, exactly as there is none in LSP, Neovim, or VS Code.

## Take / don't take

**Taken now:**
- Publish-complete-set, replace-by-owner (already had it).
- The producer reports coordinates, the framework addresses them.
- A closed severity set, with a "note" level that participates in the list but not the error count —
  VS Code's `Hint`.
- Recompute-and-replace on edit, with no incremental patching.
- Squiggle for computed problems, corner tag for authored notes, colour from the theme's semantic
  slots rather than raw colours.

**Deliberately not taken:**
- **A handlers table.** Neovim's best idea, and premature here: three render sites, no third parties,
  no one asking to add a fourth way to display a problem. Revisit if a second display of the same
  diagnostics is ever wanted — that's the signal.
- **Declarative problem-matcher validators.** The right answer *if* third-party validators without a
  code boundary become a goal. Not one yet.
- **Pull diagnostics.** Solves "the producer can't tell what's on screen". One sheet, all in memory —
  the problem doesn't exist.
- **`relatedInformation`, `tags`, `code`.** Real ideas with no consumer yet. `code` is the one to add
  first, when quick fixes land — it's the join key that lets a fix provider filter the diagnostics it
  was handed instead of re-running validators.
- **Anything from `Position`.** UTF-16 offsets would be an imported bug.

## Open, and worth deciding before it bites

**Row identity.** `Location.row` is a positional index. Neovim shipped exactly this, then moved to
extmarks, and currently has *two sources of truth for a diagnostic's position in the same public
API*. If qrate gets row reordering, resolve `row` → `dataset_main._row_id` **before** shipping it.
The `ponytail:` comment on that field is now backed by someone else's live bug.

**Debouncing.** Full re-validation runs on commit (Enter/blur), not per keystroke, so there is no
debounce and shouldn't be one yet. If it ever needs one, copy the shape rather than inventing:
coalesce by overwrite, keep only the latest pending run, and prefer cancellation over delay.

---

## Sources

**LSP** — [3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
· [overview](https://microsoft.github.io/language-server-protocol/overviews/lsp/overview/)
· [VS Code Language Server Extension Guide](https://code.visualstudio.com/api/language-extensions/language-server-extension-guide)

**Neovim** — [`:help diagnostic`](https://neovim.io/doc/user/diagnostic.html)
· [`runtime/lua/vim/diagnostic.lua`](https://github.com/neovim/neovim/blob/master/runtime/lua/vim/diagnostic.lua)
· [handlers PR #16137](https://github.com/neovim/neovim/pull/16137)
· [extmark positioning PR #34014](https://github.com/neovim/neovim/pull/34014)
· [position-incoherence issue #35136](https://github.com/neovim/neovim/issues/35136)
· [nvim-lint](https://github.com/mfussenegger/nvim-lint)
· [`:help remote_plugin`](https://neovim.io/doc/user/remote_plugin.html)

**VS Code** — [API reference](https://code.visualstudio.com/api/references/vscode-api)
· [Programmatic Language Features](https://code.visualstudio.com/api/language-extensions/programmatic-language-features)
· [Extension Host](https://code.visualstudio.com/api/advanced-topics/extension-host)
· [Contribution Points](https://code.visualstudio.com/api/references/contribution-points)
· [Tasks Appendix (problem matchers)](https://code.visualstudio.com/docs/reference/tasks-appendix)
· [`code-actions-sample`](https://github.com/microsoft/vscode-extension-samples/blob/main/code-actions-sample/src/diagnostics.ts)

**Others** — [Zed `extension.wit`](https://github.com/zed-industries/zed/tree/main/crates/extension_api/wit)
· [Zed Decoded: Extensions](https://zed.dev/blog/zed-decoded-extensions)
· [Helix plugin discussion #3806](https://github.com/helix-editor/helix/discussions/3806)
· [Helix Steel PR #8675](https://github.com/helix-editor/helix/pull/8675)
· [rust-analyzer architecture](https://rust-analyzer.github.io/book/contributing/architecture.html)
· [NullDeref: plugins in Rust](https://nullderef.com/blog/plugin-tech/)
· [Lapce PSP #558](https://github.com/lapce/lapce/issues/558)
· [Plugin systems: when & why?](https://dev.to/arcanis/plugin-systems-when-why-58pp)
