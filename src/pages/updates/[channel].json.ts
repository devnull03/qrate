import type { APIRoute } from 'astro';
import { createPublicKey, verify } from 'node:crypto';
import semver from 'semver';
import { getReleases } from '../../lib/releases.js';

export const prerender = true;

// qrate embeds this key's other half; scripts/provision-update-key.mjs in the app repo patches
// both copies at once. Verifying here too means an unsigned or tampered manifest fails the
// deployment rather than becoming an update every installed app has to reject.
const publicKey = createPublicKey({
  key: {
    kty: 'OKP',
    crv: 'Ed25519',
    x: 'cA9rIQHTbRBPFeFNLsNH31Q4PQLIHFa95c-ejSE1nGE',
  },
  format: 'jwk',
});

// A channel only gets a file once it has a signed release. Before that the URL 404s, which is
// already how the app reads "no update for this channel" — so there is no half-valid feed to
// serve and nothing to special-case in the endpoint below.
export async function getStaticPaths() {
  const signed = (await getReleases())
    .map((release) => ({ release, version: semver.parse(release.tag_name.replace(/^v/, '')) }))
    .filter(({ release, version }) => !release.draft && version)
    .filter(({ release }) => release.assets.some((a) => a.name === 'update-manifest.json'))
    .sort((a, b) => semver.rcompare(a.version, b.version));

  return ['beta', 'stable']
    .map((channel) => ({
      params: { channel },
      props: signed.find(({ version }) => channel === 'beta' || version.prerelease.length === 0),
    }))
    .filter(({ props }) => props);
}

export const GET: APIRoute = async ({ params, props }) => {
  const { release, version } = props;
  const asset = release.assets.find((a) => a.name === 'update-manifest.json');

  const response = await fetch(asset.browser_download_url);
  if (!response.ok) throw new Error(`GitHub ${response.status} fetching update manifest`);
  const envelope = await response.json();
  if (envelope.schema !== 1 || envelope.key_id !== 'qrate-update-1') {
    throw new Error(`Unsupported update envelope in ${release.tag_name}`);
  }
  const payload = Buffer.from(envelope.payload_base64, 'base64');
  const signature = Buffer.from(envelope.signature_base64, 'base64');
  if (!verify(null, payload, publicKey, signature)) {
    throw new Error(`Invalid update signature in ${release.tag_name}`);
  }
  const signed = JSON.parse(payload.toString('utf8'));
  if (signed.version !== version.version) {
    throw new Error(`Signed manifest is not ${release.tag_name}`);
  }
  if (params.channel === 'stable' && signed.channel !== 'stable') {
    throw new Error('A prerelease manifest was selected for the stable channel');
  }

  return new Response(`${JSON.stringify(envelope, null, 2)}\n`, {
    headers: {
      'Content-Type': 'application/json; charset=utf-8',
      'Cache-Control': 'public, max-age=300, must-revalidate',
    },
  });
};
