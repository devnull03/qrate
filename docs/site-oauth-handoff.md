# qrate-site — handoff for the Cloudflare move and the Google routes

Written 2026-08-15, for whoever (or whichever agent) works on `qrate-site` — the `site` branch of
this repo, checked out separately at `../qrate-site`. It is an Astro site, currently static on
GitHub Pages at `https://devnull03.github.io/qrate` with `base: '/qrate'`, holding
`index`, `get-started`, `changelog`, `license`, `privacy` and `terms`.

The desktop app now needs two things from it that a static GitHub Pages build cannot give:

| Route | Why the site has to serve it |
|---|---|
| `GET /oauth/config` | Needs a server to hold a secret and answer conditionally. Rotating the Google Cloud project becomes a deploy instead of a qrate release. |
| `/picker` | Google's file chooser is a browser component. `drive.file` grants access **only** to files the app created or the user picked *through that chooser*, so without this page qrate can never write to a spreadsheet the user already owns. |

Neither is optional-nice-to-have: the Rust side is already written against both, and until they
exist Google sync can create new spreadsheets but not touch existing ones.

## What the app already does

- `crates/data-exchange/src/google.rs` — `fetch_config` and `begin_picker`/`Picker` are written and
  compile. `DEFAULT_CONFIG_ENDPOINT` and `DEFAULT_PICKER_PAGE` are **placeholders**:
  `https://qrate.dvnl.work/oauth/config` and `https://qrate.dvnl.work/picker`.
- `crates/app/src/google.rs` — the resolution ladder and the persisted copy.

**The one thing to send back:** the real hostname. Both constants change to whatever the Pages
project ends up on, and the Google Cloud console's referrer restriction has to match it.

## 1. Cloudflare migration

```sh
npm i @astrojs/cloudflare
```

```js
// astro.config.mjs
import cloudflare from '@astrojs/cloudflare';

export default defineConfig({
  site: 'https://qrate.dvnl.work',   // the real one, once it is decided
  adapter: cloudflare(),
  output: 'static',                  // per-route opt-out below; everything else stays prerendered
  trailingSlash: 'ignore',
  vite: { plugins: [tailwindcss()] },
});
```

Notes that will bite otherwise:

- **`base: '/qrate'` goes away.** Every internal link and asset currently routes through it. Drop it
  and sweep the source for `import.meta.env.BASE_URL` and any hand-written `/qrate/…` before
  deploying, or half the site 404s.
- The release list is fetched **at build time** with the workflow's `GITHUB_TOKEN`
  (see `docs/SETUP.md` §4). Cloudflare Pages builds have no such token — either keep building on
  GitHub Actions and deploy the output with `wrangler pages deploy`, or give the Pages build a
  GitHub token of its own. The first is less to explain.
- `redeploy-site-on-release.yml` on `main` triggers the site rebuild when a release is published.
  Whatever replaces `deploy-site.yml` has to keep the same `workflow_dispatch` handle, or publishing
  a release silently stops updating the site.
- Keep `/privacy` and `/terms` at exactly those paths. Google's consent screen points at them, and
  changing a URL Google has on file means going back through the consent-screen form.

## 2. `GET /oauth/config`

The contract, in full. Anyone should be able to serve this and point a qrate at it.

```
GET /oauth/config
  Authorization: Bearer <token baked into the qrate build>
  If-None-Match: "<etag>"            (only after the first successful fetch)

→ 200 {"client_id": "…", "client_secret": "…"}   ETag: "…"
→ 304                                             ETag: "…"
→ 401                                             (wrong or missing bearer)
```

`client_secret` may be omitted; the app treats it as absent rather than empty.

```ts
// src/pages/oauth/config.ts
import type { APIRoute } from 'astro';

export const prerender = false;

export const GET: APIRoute = async ({ request, locals }) => {
  const env = locals.runtime.env;
  if (request.headers.get('authorization') !== `Bearer ${env.QRATE_GOOGLE_CONFIG_TOKEN}`) {
    return new Response(null, { status: 401 });
  }
  const body = JSON.stringify({
    client_id: env.GOOGLE_CLIENT_ID,
    client_secret: env.GOOGLE_CLIENT_SECRET,
  });
  // Derived from the body, so rotating a secret changes the ETag with nothing to remember.
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(body));
  const etag = `"${[...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('')}"`;

  if (request.headers.get('if-none-match') === etag) {
    return new Response(null, { status: 304, headers: { ETag: etag } });
  }
  return new Response(body, {
    headers: { 'content-type': 'application/json', ETag: etag },
  });
};
```

Secrets via `wrangler secret put GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` /
`QRATE_GOOGLE_CONFIG_TOKEN`. Never in committed source, never in `wrangler.toml`'s `[vars]`.

`QRATE_GOOGLE_CONFIG_TOKEN` does not exist anywhere yet — invent one (`openssl rand -base64 32`)
and put the *same* string in two places: this Worker secret, and the `QRATE_GOOGLE_CONFIG_TOKEN`
build variable qrate compiles in (`GOOGLE_CONFIG_TOKEN` as a GitHub Actions secret; see
`docs/SETUP.md` §3.6). Omitting `GOOGLE_CLIENT_SECRET` is fine for a client that has none — the
route leaves the field out rather than sending `""`, and the app reads that as absent.

A mismatch is silent: qrate gets a 401, keeps its compiled-in pair, and logs at `warn`. So check it
once by hand after deploying, which also proves the ETag path:

```sh
curl -isS -H "Authorization: Bearer $TOKEN" https://<site>/oauth/config      # 200 + ETag
curl -isS -H "Authorization: Bearer $TOKEN" -H 'If-None-Match: "<etag>"' \
  https://<site>/oauth/config                                                # 304
```

**Be accurate about the bearer.** It ships inside the qrate binary, so it stops casual scraping and
drive-by indexing and nothing else. Do not describe it as access control in code comments or on the
site — and it does not need to be, because Google treats an installed app's client secret as
non-confidential (RFC 8252 §8.5). Loopback + PKCE is what secures the exchange.

**This must never grow into a token-exchange proxy.** Google's redirect has to reach the app on the
user's own machine. Anything in the middle either keeps the loopback listener anyway, asks the user
to paste a code, or needs a session store — and all three put user Drive tokens on our
infrastructure. The Worker serves *application* credentials and nothing else.

A 401, a 500, or an unreachable host all land in the same branch on the app side: keep whatever is
stored, log at `warn`, carry on. Sign-in never blocks on this endpoint.

## 3. `/picker`

A prerendered page — no server involvement at all. qrate opens it with the access token, the
nonce, and the loopback port in the URL **fragment**, which browsers never send to a server:

```
https://<site>/picker#token=<access_token>&state=<nonce>&port=<loopback port>
```

The page must, after the user picks, redirect to:

```
http://127.0.0.1:<port>/?fileId=<spreadsheet id>&state=<the same nonce, echoed back>
```

qrate rejects the reply if `state` does not match, so echoing it is not optional. A user who closes
the window without picking simply never redirects; qrate's listener stays blocked until the app
closes, which is the same shape as an abandoned consent.

```js
const p = new URLSearchParams(location.hash.slice(1));
// Don't leave an access token in browser history.
history.replaceState(null, '', location.pathname);

gapi.load('picker', () => {
  new google.picker.PickerBuilder()
    .setAppId(APP_ID)                 // "805791669854" — the Cloud project number
    .setOAuthToken(p.get('token'))
    .setDeveloperKey(PICKER_API_KEY)  // a browser API key, referrer-restricted to this site
    .addView(new google.picker.DocsView(google.picker.ViewId.SPREADSHEETS))
    .setCallback((data) => {
      if (data.action !== google.picker.Action.PICKED) return;
      location.replace(
        `http://127.0.0.1:${p.get('port')}/?fileId=${encodeURIComponent(data.docs[0].id)}` +
          `&state=${encodeURIComponent(p.get('state'))}`,
      );
    })
    .build()
    .setVisible(true);
});
```

`setAppId` is the load-bearing line. Picking a file through a Picker configured with the same Cloud
project number is exactly what grants that file to a `drive.file` token — without it the user picks
something and qrate still gets a 404 on write. The API key is a browser key, safe to ship in the
page, and should be restricted by HTTP referrer to this site.

The page needs no styling beyond a heading; it exists for about four seconds.

## 4. Google Cloud console — still to be done by a human

Not the site agent's job, but the routes above are inert without it:

- Enable the **Sheets**, **Drive** and **Picker** APIs on project `805791669854`.
- Create a **browser API key**, referrer-restricted to the site, for `setDeveloperKey`.
- Fill the OAuth consent screen, including the `/privacy` and `/terms` URLs, and move it from
  Testing to Production. Until then refresh tokens expire after 7 days and the app caps at 100 users.
- Scope stays `drive.file` — non-sensitive, no brand-verification review. Widening it to
  `spreadsheets` is a decision that was considered and rejected; see `docs/google-sync-handoff.md`.

## 5. Self-hosting is a requirement

The Worker source, this contract, and the setting that points somewhere else are all public. An
institution that will not route its staff through our endpoint sets **Settings ▸ Google ▸ Credential
endpoint** to their own and runs the same flow against their own Cloud project — no fork, no Rust.
Whatever ends up in `qrate-site` should be readable as a template for exactly that.
