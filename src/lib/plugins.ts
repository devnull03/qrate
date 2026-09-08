import { createHash, createPublicKey, verify } from 'node:crypto';

export const REGISTRY_REPO = 'devnull03/qrate-plugin-registry';
export const TEMPLATE_REPO = 'devnull03/qrate-plugin-template';

export type PluginRelease = {
  version: string;
  api_version: number;
  permissions: string[];
  published_at: string;
  release_url: string;
  artifact_url: string;
  sha256: string;
  bytes: number;
  status: 'active' | 'revoked';
  revocation_reason?: string;
};

export type Plugin = {
  id: string;
  name: string;
  summary: string;
  description: string;
  categories: string[];
  license: string;
  publisher: string;
  repository: string;
  homepage?: string;
  support?: string;
  icon?: string;
  screenshots: string[];
  featured: boolean;
  current: PluginRelease;
};

export type PluginCatalog = {
  configured: boolean;
  generated_at?: string;
  source_commit?: string;
  plugins: Plugin[];
};

type Signature = {
  schema: number;
  key_id: string;
  algorithm: string;
  sha256: string;
  signature_base64: string;
};

let catalog: Promise<PluginCatalog> | undefined;
const MAX_CATALOG_BYTES = 5 * 1024 * 1024;
const CATALOG_PUBLIC_KEY = 'FSZ_woODWVPRGLOa6lH3e8tJFovJUnkol-SIo-PGVWU';
const PLUGIN_ID = /^[a-z0-9](?:[a-z0-9_-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9_-]*[a-z0-9])?)+$/;
const SHA256 = /^[a-f0-9]{64}$/i;
const ACCEPTED_LICENSES = new Set([
  'Apache-2.0',
  'BSD-2-Clause',
  'BSD-3-Clause',
  'CC-BY-4.0',
  'GPL-3.0-only',
  'GPL-3.0-or-later',
  'LGPL-3.0-only',
  'LGPL-3.0-or-later',
  'MIT',
  'Unlicense',
  'Zlib',
]);

const text = (value: unknown, field: string) => {
  if (typeof value !== 'string' || value.trim() === '') throw new Error(`Plugin catalog has no ${field}`);
  return value;
};

const optionalText = (value: unknown, field: string) => {
  if (value == null) return undefined;
  return text(value, field);
};

const strings = (value: unknown, field: string) => {
  if (!Array.isArray(value) || value.some((item) => typeof item !== 'string')) {
    throw new Error(`Plugin catalog ${field} must be a string array`);
  }
  return value;
};

const url = (value: unknown, field: string) => {
  const parsed = new URL(text(value, field));
  if (parsed.protocol !== 'https:') throw new Error(`Plugin catalog ${field} must use HTTPS`);
  return parsed.href;
};

const parsePlugin = (value: unknown): Plugin => {
  if (!value || typeof value !== 'object') throw new Error('Plugin catalog contains an invalid record');
  const item = value as Record<string, unknown>;
  const release = item.current;
  if (!release || typeof release !== 'object') throw new Error(`Plugin catalog ${item.id} has no current release`);
  const current = release as Record<string, unknown>;
  const status = current.status;
  if (status !== 'active' && status !== 'revoked') {
    throw new Error(`Plugin catalog ${item.id} has an invalid release status`);
  }

  const id = text(item.id, 'plugin ID');
  if (!PLUGIN_ID.test(id)) throw new Error(`Plugin catalog has an invalid plugin ID: ${id}`);

  const publishedAt = text(current.published_at, 'publication date');
  if (Number.isNaN(Date.parse(publishedAt))) {
    throw new Error(`Plugin catalog ${id} has an invalid publication date`);
  }
  const apiVersion = current.api_version;
  if (!Number.isInteger(apiVersion) || (apiVersion as number) < 1 || (apiVersion as number) > 2) {
    throw new Error(`Plugin catalog ${id} needs an unsupported API version`);
  }
  const permissions = strings(current.permissions, 'permissions');
  if (new Set(permissions).size !== permissions.length || permissions.some((permission) => permission !== 'net')) {
    throw new Error(`Plugin catalog ${id} has invalid permissions`);
  }
  const license = text(item.license, 'license');
  if (!ACCEPTED_LICENSES.has(license)) {
    throw new Error(`Plugin catalog ${id} uses an unsupported license`);
  }
  const sha256 = text(current.sha256, 'package SHA-256');
  if (!SHA256.test(sha256)) throw new Error(`Plugin catalog ${id} has an invalid package SHA-256`);
  const bytes = current.bytes;
  if (!Number.isSafeInteger(bytes) || (bytes as number) < 0 || (bytes as number) > 100 * 1024 * 1024) {
    throw new Error(`Plugin catalog ${id} has an invalid package size`);
  }

  return {
    id,
    name: text(item.name, 'plugin name'),
    summary: text(item.summary, 'plugin summary'),
    description: text(item.description, 'plugin description'),
    categories: strings(item.categories, 'categories'),
    license,
    publisher: text(item.publisher, 'publisher'),
    repository: url(item.repository, 'repository'),
    homepage: item.homepage == null ? undefined : url(item.homepage, 'homepage'),
    support: item.support == null ? undefined : url(item.support, 'support URL'),
    icon: item.icon == null ? undefined : url(item.icon, 'icon'),
    screenshots: strings(item.screenshots ?? [], 'screenshots').map((screenshot) =>
      url(screenshot, 'screenshot'),
    ),
    featured: item.featured === true,
    current: {
      version: text(current.version, 'release version'),
      api_version: apiVersion as number,
      permissions,
      published_at: publishedAt,
      release_url: url(current.release_url, 'release URL'),
      artifact_url: url(current.artifact_url, 'artifact URL'),
      sha256: sha256.toLowerCase(),
      bytes: bytes as number,
      status,
      revocation_reason: optionalText(current.revocation_reason, 'revocation reason'),
    },
  };
};

async function loadCatalog(): Promise<PluginCatalog> {
  const url =
    process.env.QRATE_PLUGIN_CATALOG_URL ?? 'https://qrate.dvnl.work/plugins/catalog.json';
  const publicKey = process.env.QRATE_PLUGIN_CATALOG_PUBLIC_KEY ?? CATALOG_PUBLIC_KEY;

  const [catalogResponse, signatureResponse] = await Promise.all([
    fetch(url, { signal: AbortSignal.timeout(15_000) }),
    fetch(`${url}.sig`, { signal: AbortSignal.timeout(15_000) }),
  ]);
  if (!catalogResponse.ok) throw new Error(`Plugin catalog returned HTTP ${catalogResponse.status}`);
  if (!signatureResponse.ok) {
    throw new Error(`Plugin catalog signature returned HTTP ${signatureResponse.status}`);
  }

  const bytes = Buffer.from(await catalogResponse.arrayBuffer());
  if (bytes.byteLength > MAX_CATALOG_BYTES) throw new Error('Plugin catalog is too large');
  const signature = (await signatureResponse.json()) as Signature;
  if (
    signature.schema !== 1 ||
    signature.key_id !== 'qrate-plugin-catalog-1' ||
    signature.algorithm !== 'Ed25519'
  ) {
    throw new Error('Plugin catalog has an unsupported signature');
  }

  const digest = createHash('sha256').update(bytes).digest('hex');
  if (digest !== signature.sha256.toLowerCase()) throw new Error('Plugin catalog hash does not match');

  const key = createPublicKey({
    key: { kty: 'OKP', crv: 'Ed25519', x: publicKey },
    format: 'jwk',
  });
  if (!verify(null, bytes, key, Buffer.from(signature.signature_base64, 'base64'))) {
    throw new Error('Plugin catalog signature did not verify');
  }

  const parsed = JSON.parse(bytes.toString('utf8')) as Record<string, unknown>;
  if (parsed.schema !== 1 || !Array.isArray(parsed.plugins)) {
    throw new Error('Plugin catalog has an unsupported schema');
  }

  const plugins = parsed.plugins.map(parsePlugin);
  const ids = new Set<string>();
  for (const plugin of plugins) {
    if (ids.has(plugin.id)) throw new Error(`Plugin catalog repeats ${plugin.id}`);
    ids.add(plugin.id);
  }

  return {
    configured: true,
    generated_at: optionalText(parsed.generated_at, 'generation date'),
    source_commit: optionalText(parsed.source_commit, 'source commit'),
    plugins,
  };
}

export const getPluginCatalog = () => (catalog ??= loadCatalog());

export const installLink = (id: string) =>
  `qrate://plugin/install?source=registry&id=${encodeURIComponent(id)}`;
