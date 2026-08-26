#!/usr/bin/env node
// Generate the update signing key, store the private half as the `release-signing` environment
// secret, and patch the two public copies (the Rust updater and the site's feed route).
//
//   node scripts/provision-update-key.mjs [path-to-qrate-site]
//
// The private key is piped straight into `gh` and is never written to disk or printed.

import { execFileSync } from 'node:child_process';
import { generateKeyPairSync } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const sitePath = path.resolve(process.argv[2] ?? path.join(repoRoot, '..', 'qrate-site'));

const { publicKey, privateKey } = generateKeyPairSync('ed25519');
const raw = publicKey.export({ format: 'jwk' });
const bytes = Buffer.from(raw.x, 'base64url');
if (bytes.length !== 32) throw new Error('unexpected Ed25519 public key length');

const rustPath = path.join(repoRoot, 'crates', 'updater', 'src', 'lib.rs');
const rustArray = Array.from(bytes, (b) => `0x${b.toString(16).padStart(2, '0')}`)
  .reduce((rows, hex, i) => {
    if (i % 14 === 0) rows.push([]);
    rows.at(-1).push(hex);
    return rows;
  }, [])
  .map((row) => `    ${row.join(', ')},`)
  .join('\n');
const rust = readFileSync(rustPath, 'utf8').replace(
  /(const UPDATE_PUBLIC_KEY: \[u8; 32\] = \[\n)[\s\S]*?(\n\];)/,
  `$1${rustArray}$2`,
);
writeFileSync(rustPath, rust);
console.log(`patched ${path.relative(repoRoot, rustPath)}`);

const feedPath = path.join(sitePath, 'src', 'pages', 'updates', '[channel].json.ts');
if (existsSync(feedPath)) {
  const feed = readFileSync(feedPath, 'utf8').replace(/(\n\s+x: ')[A-Za-z0-9_-]+(',)/, `$1${raw.x}$2`);
  writeFileSync(feedPath, feed);
  console.log(`patched ${feedPath}`);
} else {
  console.warn(`no qrate-site checkout at ${sitePath}; patch its feed route with x = ${raw.x}`);
}

execFileSync('gh', ['api', '-X', 'PUT', 'repos/devnull03/qrate/environments/release-signing'], {
  stdio: ['ignore', 'ignore', 'inherit'],
});
execFileSync(
  'gh',
  ['secret', 'set', 'QRATE_UPDATE_SIGNING_KEY', '--env', 'release-signing', '--body-file', '-'],
  { input: privateKey.export({ type: 'pkcs8', format: 'pem' }), stdio: ['pipe', 'inherit', 'inherit'] },
);
console.log('stored QRATE_UPDATE_SIGNING_KEY in the release-signing environment');
