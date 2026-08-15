import type { APIRoute } from 'astro';
// Astro 6 removed `locals.runtime.env`; secrets come off the Workers binding.
import { env } from 'cloudflare:workers';

// The only route on the site that isn't prerendered.
export const prerender = false;

/**
 * Google's client id and secret for the desktop app, so rotating the Cloud
 * project is a deploy here instead of a qrate release.
 *
 * The bearer token ships inside every qrate binary. It stops casual scraping
 * and indexing and nothing else — it is not access control, and does not need
 * to be: Google treats an installed app's client secret as non-confidential
 * (RFC 8252 §8.5), and loopback + PKCE is what secures the exchange.
 *
 * This must never grow into a token-exchange proxy. Google's redirect has to
 * reach the app on the user's own machine; anything in the middle would put
 * user Drive tokens on our infrastructure. Application credentials only.
 *
 * The contract is qrate's `fetch_config` in crates/data-exchange/src/google.rs.
 */
export const GET: APIRoute = async ({ request }) => {
  if (request.headers.get('authorization') !== `Bearer ${env.QRATE_GOOGLE_CONFIG_TOKEN}`) {
    return new Response(null, { status: 401 });
  }

  // Omitted rather than empty when unset — `client_secret` is an Option on the
  // app side, and Google does not require one for an installed app.
  const body = JSON.stringify({
    client_id: env.GOOGLE_CLIENT_ID,
    ...(env.GOOGLE_CLIENT_SECRET ? { client_secret: env.GOOGLE_CLIENT_SECRET } : {}),
  });

  // Derived from the body, so rotating a secret changes the ETag with nothing
  // to remember.
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(body));
  const etag = `"${[...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('')}"`;

  if (request.headers.get('if-none-match') === etag) {
    return new Response(null, { status: 304, headers: { ETag: etag } });
  }

  return new Response(body, {
    headers: { 'content-type': 'application/json', ETag: etag },
  });
};
