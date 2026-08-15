# Google Sheets sync — handoff

Written 2026-08-15. Context for whoever (or whichever agent) picks up ASNT-93 through ASNT-96 —
the existing-spreadsheet work, the credential endpoint, the keychain move, and the opt-in gate.
Those four are one body of work and are meant to be done together. The per-task instructions live
in Notion; this file holds only what is shared between them and would otherwise have to be
rediscovered by reading the OAuth code.

> **Status, 2026-08-15.** The Rust side of all four is written, and `qrate.dvnl.work` serves both
> `/oauth/config` and `/picker` (see `docs/site-oauth-handoff.md`). **ASNT-93's open decision is
> closed: Route A** — `drive.file` stays, and the Picker is how a user points qrate at a
> spreadsheet they already own.
>
> Nothing Google-facing works end to end yet, and all three remaining pieces are outside this repo:
> the Cloud console work (enable Sheets/Drive/Picker, a referrer-restricted Picker API key, consent
> screen out of Testing), the three GitHub Actions secrets, and a deploy hook to replace the
> release→site dispatch the Cloudflare cutover broke (`docs/SETUP.md` §4).
>
> Both the config token and the client secret were pasted into agent transcripts while setting this
> up. Neither is confidential by design, but rotating them costs one Cloudflare secret each *until
> the first binary ships with them baked in* — after that it costs a release. Cheap now.

There are no existing installs, so nothing here needs a migration path. Where the current code
does the wrong thing, replace it rather than adding a fallback for users who do not exist.

## What already exists

Far more than the disabled menu item suggests. All of this is written and tested:

| Piece | Where |
|---|---|
| OAuth: loopback + PKCE, consent, token exchange, refresh | `crates/data-exchange/src/google.rs` |
| Sheets write: create a spreadsheet, fill tab 1 | `google.rs::create_sheet` |
| Export action, background threading, token storage | `crates/app/src/export.rs::to_google_sheet` |
| Sheet **import** (public link, xlsx, incl. cell notes) | `crates/data-exchange/src/sheet.rs` |
| Credential ladder, keychain, the opt-in read | `crates/app/src/google.rs` |
| Picker round trip, `write_values`, the config fetch | `google.rs::begin_picker`, `write_values`, `fetch_config` |

The Google entries are no longer disabled — they are **absent** until the user switches Google sync
on in Settings ▸ Google, which is also where they read what they are agreeing to.

## How login works today

1. `begin_consent()` binds `127.0.0.1:0` **before** anything opens, generates a PKCE verifier and a
   `state` nonce, and builds the auth URL with `redirect_uri=http://127.0.0.1:<port>`.
2. gpui's `cx.open_url` hands that to the user's real browser, so they consent on a genuine
   `accounts.google.com` page with their existing session.
3. Google redirects to the loopback port. `Consent::wait_for_token()` reads the code from the
   request line, checks `state`, writes back a small HTML page, and exchanges the code.
4. The refresh token is stored user-wide, so consent happens once per machine.

This is RFC 8252 §7.3 — the loopback redirect is Google's *recommended* flow for installed apps,
not a workaround. Two consequences worth writing down because they keep getting re-litigated:

**No auth proxy.** Google's redirect has to reach the app on the user's machine. A server in the
middle either keeps the loopback listener anyway (an extra hop, with the port smuggled through
`state`), asks the user to paste a code (the deprecated OOB pattern), or needs a session store the
app polls. All three are worse, and all three put user Drive tokens on our infrastructure.
ASNT-94 moves *credential delivery* to a Worker and nothing else — keep it that way.

**The baked-in client secret is not a leak.** Google treats an installed app's secret as
non-confidential (RFC 8252 §8.5). Loopback + PKCE is what secures the exchange; extracting the
secret lets someone build an app whose consent screen says "qrate", which is a branding concern,
not a credential one.

## Two different secrets — don't conflate them

Half the confusion in this area comes from calling both of these "the Google credentials".

| | **Client credentials** (id + secret) | **Refresh token** |
|---|---|---|
| Identifies | the qrate application | one user's grant on their Drive |
| Same for | every user | nobody — it's per person, per machine |
| If disclosed | someone can imitate our consent screen | someone has that user's Drive, indefinitely |
| Belongs in | `AppSettings` — ASNT-94 | the OS credential store — ASNT-95 |

ASNT-94 deliberately *persists* the client credentials to disk. ASNT-95 deliberately *removes*
the refresh token from that same file. Those aren't contradictory; they're the table above.

## Credential delivery (ASNT-94)

Not a per-launch fetch. The app keeps its own persisted copy and only asks the endpoint whether
that copy is stale.

```
GET /oauth/config                     Authorization: Bearer <baked-in token>
  → 200 {client_id, client_secret}    ETag: "…"
  → 304                               (If-None-Match matched — nothing to do)
```

Client stores `{client_id, client_secret, etag, checked_at}` and resolves in this order:

1. the persisted copy, if `checked_at` is recent (~7 days)
2. otherwise send a conditional request, then the persisted copy
3. the compiled-in `option_env!` constants, if nothing was ever persisted

The check rides the first Google action that needs it — no background timer, no startup fetch. A
network failure keeps whatever is stored and logs at `warn`; it never blocks an export, because
working offline is the normal case for this audience.

About the bearer token: it is shipped inside the binary, so it stops casual scraping and drive-by
indexing and nothing more. Don't describe it as access control in code comments or docs — the
whole point of the section above is that the thing it guards isn't confidential anyway.

## Self-hosting is a requirement, not a nice-to-have

The Worker source, the endpoint contract, and the setting that points somewhere else are all
public. Anyone should be able to run this flow on their own infrastructure against their own
Google Cloud project, without reading Rust to work out what to serve.

Concretely that means: a user-wide setting for the config endpoint URL (defaulting to ours), the
contract documented in `docs/`, and the Worker plus its deploy steps in `qrate-site` next to the
policy pages. An institution that won't route its staff through our endpoint has a supported
answer rather than a fork.

The setting exists (Settings ▸ Google ▸ Credential endpoint) and the contract is written down in
`docs/site-oauth-handoff.md`. The Worker itself is the site agent's half.

## The one structural blocker

qrate has no stable row identity. `save_dataset` (`crates/settings/src/project.rs:640`) drops and
recreates `dataset_main` on every save, so `_row_id` is reassigned by insertion order. Notes are
keyed by the same positional index.

Push-only sync does not care. Anything that **reads** from the Sheet does, because there is no way
to say which qrate row a returned Sheet row corresponds to. That is why ASNT-84 exists and why
ASNT-85 (pull) is blocked on it.

## Scope tiers, since every design question lands here

| Scope | Tier | Cost |
|---|---|---|
| `drive.file` — files the app created or the user picked | non-sensitive | none |
| `spreadsheets` — every spreadsheet in the account | sensitive | brand verification review |
| `drive`, `drive.readonly` | restricted | verification + annual CASA assessment (billed in thousands) |

We are on `drive.file`, and staying there. Note that a spreadsheet **ID is not authorization** under
`drive.file` — a file the token was never granted returns 404, not 403. That is why ASNT-93 ships
the Picker rather than a URL box, and why `write_values` maps a 404 to `GoogleError::NoAccess`
instead of letting a bare status reach the user.

Also: the consent screen must move from Testing to Production before release, or refresh tokens
expire after 7 days and the app caps at 100 users. Production needs hosted privacy policy and
terms URLs — those are being written into `qrate-site` separately.

## Suggested order

```
ASNT-80  create the OAuth client, switch the menu on        ← gates everything
   ├── ASNT-94  credential endpoint + self-hosting          (1–2 days, incl. Worker and docs)
   ├── ASNT-95  refresh token into the OS keychain          (half a day)
   └── ASNT-93  sync into an existing spreadsheet
          ├── ASNT-83  linked push sync
          └── ASNT-96  the opt-in gate + consent dialog
ASNT-84  stable row identity                                 (independent; also wanted by
   └── ASNT-85  authenticated read and reviewed pull          merge/explode rows and undo)
```

94, 95 and 96 all touch `AppSettings` and the same Settings section, so doing them in one pass is
cheaper than three: the opt-in toggle, the config endpoint field, and the keychain swap land
together.

ASNT-93's open decision (Picker vs. widening the scope) was answered: Route A, the Picker.

## Open questions the design left for the user

1. Is one-way push honest enough for real users, or does the first release need to read back?
2. Push on save, or on a slower timer?
3. Does the sync own the whole tab, or a named range?

Full option analysis: <https://claude.ai/code/artifact/a17244b7-ccdd-48dd-bd39-7e966ea7b751>

## Working reminders

- `QRATE_GOOGLE_CLIENT_ID` / `_SECRET` / `_CONFIG_TOKEN` are build-time `option_env!`, and stay as
  the last-resort fallback even after ASNT-94. **Never commit the credential JSON or the values.**
  `.gitignore` now covers `client_secret_*.json`; the values themselves belong in the environment
  (`docs/SETUP.md` §2) or in Actions secrets, never in source.
- CI is `cargo fmt --all --check`, then `clippy --workspace --all-targets -- -D warnings -A dead_code`,
  then `cargo test --workspace`. All three green before any PR.
- No `Co-Authored-By: Claude` trailer on commits, ever.
