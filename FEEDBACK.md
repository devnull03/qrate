# Feedback deployment

Desktop branch: `feat/in-app-feedback`. Website branch: `feat/feedback-site`.
Deploy the website before releasing the desktop buttons.

## Configure

1. Create a Cloudflare Turnstile widget for `qrate.dvnl.work`.
2. Set Worker variable `TURNSTILE_SITE_KEY` to its public site key.
3. Run `bunx wrangler secret put TURNSTILE_SECRET_KEY`.
4. Run `bunx wrangler secret put LINEAR_API_KEY`.

The Linear key needs read access, issue creation and file upload access for Grass Labs.
Never put either secret in source files, command arguments, or the browser.
Routing IDs live in `src/lib/feedback.ts`. New issues go to qrate / Beta Intake & Stabilization / Backlog.

## Test locally

1. Copy `.dev.vars.example` to `.dev.vars`.
2. Put the Linear key in `.dev.vars`.
3. Keep Cloudflare's public test Turnstile keys from the example file.
4. Run `bun install`.
5. Run `bun run dev`.
6. Open `http://localhost:4321/feedback`.
7. In another checkout on `feat/in-app-feedback`, run `cargo run`.

Debug qrate builds open the local form. Release builds open `https://qrate.dvnl.work/feedback`.
The repository ignores `.dev.vars`. Stop the local server before switching site branches.
Cloudflare test keys work on local loopback addresses only. Production requires a real Turnstile widget.

## Verify before deployment

- Run `bun test src/lib/feedback.test.ts`.
- Run `bun --bun run build`.
- Run Wrangler preview with local secrets supplied through the Cloudflare-supported local environment.
- Verify Turnstile hostname and action checks using the preview's hostname.
- Submit a synthetic report with no private data. Verify routing and labels in Linear.
- Submit a text attachment and screenshot. Verify private attachment access and Logs Attached.
- Simulate a lost response and retry the same UUID. Verify only one issue exists.
- Confirm no analytics beacon appears on `/feedback`.
- Confirm the browser clears the fragment immediately and diagnostics are editable.
- Test the title-bar button in packaged Windows, macOS, and Linux builds.

## Operations and limits

Five attempts per minute per Cloudflare location/IP. Workplace users may share an IP.
Three user attachments, up to 5 MiB each. PNG/JPEG screenshots and TXT/LOG logs only.
The optional automatic session log is shown inline in Linear up to 64 KiB. Larger logs become
gzip attachments in browsers that support compression and have a separate 512 KiB limit.
No automatic crash uploads, session recording, screenshot capture, or full project upload.
The report UUID also serves as Linear issue UUID for retry deduplication. Retries require a new Turnstile token.
Uploads that succeed before a later provider failure can leave orphaned Linear assets.
No private report text is logged by the handler.

The public privacy policy describes purpose-based retention, not automatic timed deletion.
Confirm the team's retention/deletion process and Linear team visibility before launch.
Configure the Feedback Dashboard manually using project, milestone, active states and Feedback OR Needs Triage labels.
Remove Needs Triage when reviewed. Apply Reproducible only after verification.

Deployment and live provider verification are separate operator steps. Unit tests use mocked providers, not actual tickets.
