// Pull the user docs from `docs/` on main into the Starlight collection.
//
// main and site are branches of the same repository, so this reads through
// `git show` rather than the network: no token, no rate limit, and it works
// offline. Re-run it after docs change on main; the output is committed so CI
// builds without needing main checked out.
//
//   bun run sync-docs
//
// docs/dev/ is internal and never synced. docs/index.md is the source of truth
// for both the page list and the sidebar: the two link groups in it become the
// two sidebar groups, in the order they appear there.
//
// ponytail: no watch mode, no incremental diffing. It is 11 files.

import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync, rmSync, readFileSync, existsSync } from 'node:fs';
import { dirname, join, posix } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const OUT = join(ROOT, 'src/content/docs/docs');
const REPO = 'https://github.com/devnull03/qrate';
const REF = process.env.QRATE_DOCS_REF || 'main';

const show = (path) =>
  execFileSync('git', ['show', `${REF}:${path}`], { cwd: ROOT, encoding: 'utf8' });

// --- read the index, which defines the groups, the page order, and the blurbs

const index = show('docs/index.md');

// Each `## Group` heading owns the bullet list under it. Anything without a
// relative .md link (the "For contributors" section) is not a group of pages.
const groups = [];
let group = null;
for (const line of index.split('\n')) {
  const heading = /^##\s+(.+)$/.exec(line);
  if (heading) {
    group = { label: heading[1].trim(), items: [] };
    groups.push(group);
    continue;
  }
  const item = /^-\s+\[([^\]]+)\]\(([^)]+\.md)\)\s*(?:[—-]\s*(.*))?$/.exec(line);
  if (item && group && !item[2].startsWith('http')) {
    group.items.push({ label: item[1], src: item[2], description: (item[3] || '').trim() });
  }
}

const pages = groups.flatMap((g) => g.items);
if (!pages.length) throw new Error(`No pages found in ${REF}:docs/index.md`);

// --- link rewriting

// `docs/plugins/islandora.md` -> `/docs/plugins/islandora`, index -> its directory.
const urlFor = (rel) => {
  const clean = rel.replace(/\.md$/, '');
  return posix.join('/docs', clean === 'index' ? '' : clean.replace(/\/index$/, ''));
};

// Anything outside docs/ (AGENTS.md) or inside docs/dev/ stays on GitHub: the
// first is not a docs page, the second is deliberately unpublished.
const rewriteLinks = (body, srcDir) =>
  body.replace(/\]\(([^)]+)\)/g, (whole, href) => {
    if (/^(https?:|mailto:|#)/.test(href)) return whole;

    const [path, hash = ''] = href.split('#');
    const frag = hash ? `#${hash}` : '';
    const resolved = posix.normalize(posix.join(srcDir, path));

    if (resolved.startsWith('docs/dev')) return `](${REPO}/tree/${REF}/${resolved})`;
    if (!resolved.startsWith('docs/')) return `](${REPO}/blob/${REF}/${resolved})`;
    return `](${urlFor(resolved.slice('docs/'.length))}${frag})`;
  });

const yaml = (s) => `'${s.replace(/'/g, "''")}'`;

// --- write the pages

// Only clear what a previous run wrote. Hand-authored pages live in the same
// collection (install.mdx, reference/ai-tools.md) and must survive a re-sync,
// so a blanket rm of the output directory is not an option.
const MANIFEST = join(OUT, '.synced.json');
if (existsSync(MANIFEST)) {
  for (const rel of JSON.parse(readFileSync(MANIFEST, 'utf8'))) {
    rmSync(join(OUT, rel), { force: true });
  }
}
const written = [];

const write = (rel, title, description, body, order) => {
  written.push(rel);
  const out = join(OUT, rel);
  mkdirSync(dirname(out), { recursive: true });
  const front = [
    '---',
    `title: ${yaml(title)}`,
    description && `description: ${yaml(description)}`,
    order != null && `sidebar:\n  order: ${order}`,
    '---',
  ]
    .filter(Boolean)
    .join('\n');
  writeFileSync(out, `${front}\n\n${body.trimStart()}\n`, 'utf8');
};

// The `# Heading` becomes the frontmatter title; Starlight renders it, so
// leaving it in the body would print it twice.
const split = (raw) => {
  const m = /^#\s+(.+)\n/.exec(raw);
  return m ? { title: m[1].trim(), body: raw.slice(m[0].length) } : { title: null, body: raw };
};

pages.forEach((p, i) => {
  const src = `docs/${p.src}`;
  const { title, body } = split(show(src));
  write(p.src, title || p.label, p.description, rewriteLinks(body, posix.dirname(src)), i);
});

// api-reference.md is reachable from plugins/index.md but is not itself listed
// in docs/index.md, so it would otherwise be fetched-but-unlisted.
const extras = ['plugins/api-reference.md'];
for (const [i, rel] of extras.entries()) {
  const src = `docs/${rel}`;
  const { title, body } = split(show(src));
  write(rel, title || rel, '', rewriteLinks(body, posix.dirname(src)), pages.length + i);
  const g = groups.find((x) => x.items.some((it) => it.src.startsWith(`${rel.split('/')[0]}/`)));
  if (g) g.items.push({ label: title, src: rel });
}

// --- the docs landing page, from the index's own prose
//
// The "For contributors" section is dropped: it points at docs/dev, which is
// internal working material rather than published documentation. The user docs
// should not advertise it, so it is neither a page nor a sidebar entry here.

{
  const { title, body } = split(index);
  const trimmed = body.replace(/\n##\s+For contributors\n[\s\S]*$/, '\n');
  write('index.md', title || 'qrate docs', '', rewriteLinks(trimmed, 'docs'), null);
}

writeFileSync(MANIFEST, JSON.stringify(written, null, 2), 'utf8');

// --- the sidebar, so astro.config never drifts from index.md

writeFileSync(
  join(ROOT, 'src/sidebar.generated.js'),
  `// Generated by scripts/sync-docs.mjs from ${REF}:docs/index.md. Do not edit.\n` +
    `export default ${JSON.stringify(
      groups
        .filter((g) => g.items.length)
        .map((g) => ({
          label: g.label,
          items: g.items.map((it) => `docs/${it.src.replace(/(?:\/?index)?\.md$/, '')}`),
        })),
      null,
      2
    )};\n`,
  'utf8'
);

console.log(
  `synced ${pages.length + extras.length + 1} pages from ${REF}:docs/ into src/content/docs/docs/`
);
