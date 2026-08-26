#!/usr/bin/env node

import { createHash, createPrivateKey, sign } from 'node:crypto';
import { readdir, readFile, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';

const [distArg, tag] = process.argv.slice(2);
if (!distArg || !tag || !process.env.QRATE_UPDATE_SIGNING_KEY) {
  throw new Error('usage: QRATE_UPDATE_SIGNING_KEY=<PEM> build-update-manifest.mjs <dist> <tag>');
}
const version = tag.replace(/^v/, '');
const dist = path.resolve(distArg);
const descriptors = [
  [/-setup\.exe$/, 'windows-nsis', 'windows', 'x86_64'],
  [/-x86_64\.zip$/, 'windows-portable', 'windows', 'x86_64'],
  [/-universal\.dmg$/, 'macos-bundle', 'macos', 'universal'],
  [/-x86_64-linux\.tar\.gz$/, 'linux-tar', 'linux', 'x86_64'],
];
const artifacts = [];
for (const name of (await readdir(dist)).sort()) {
  const descriptor = descriptors.find(([pattern]) => pattern.test(name));
  if (!descriptor) continue;
  const bytes = await readFile(path.join(dist, name));
  const [, kind, os, arch] = descriptor;
  artifacts.push({
    kind,
    os,
    arch,
    url: `https://github.com/devnull03/qrate/releases/download/${tag}/${name}`,
    size: (await stat(path.join(dist, name))).size,
    sha256: createHash('sha256').update(bytes).digest('hex'),
  });
}
if (artifacts.length !== descriptors.length) {
  throw new Error(`expected ${descriptors.length} update artifacts, found ${artifacts.length}`);
}
const payload = Buffer.from(JSON.stringify({
  channel: version.includes('-') ? 'beta' : 'stable',
  version,
  published_at: new Date().toISOString(),
  release_notes_url: `https://github.com/devnull03/qrate/releases/tag/${tag}`,
  artifacts,
}));
const key = createPrivateKey(process.env.QRATE_UPDATE_SIGNING_KEY);
if (key.asymmetricKeyType !== 'ed25519') throw new Error('update signing key must be Ed25519');
const envelope = {
  schema: 1,
  key_id: 'qrate-update-1',
  payload_base64: payload.toString('base64'),
  signature_base64: sign(null, payload, key).toString('base64'),
};
await writeFile(path.join(dist, 'update-manifest.json'), `${JSON.stringify(envelope, null, 2)}\n`);
