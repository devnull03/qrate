import { readdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

/**
 * Drop the trailing slash from the URLs in the MCP corpus.
 *
 * starlight-mcp builds `https://qrate.dvnl.work/docs/columns/` itself rather
 * than asking Astro how this site formats paths, and Cloudflare 307s that back
 * to the extensionless form. Agents are the one audience that should not have to
 * follow a redirect to read a page, so the corpus is corrected in place.
 */
async function tidyMcpCorpus(root, logger) {
  const file = join(root, 'mcp', 'docs-index.json');
  let raw;
  try {
    raw = await readFile(file, 'utf8');
  } catch {
    return; // no MCP integration in this build
  }

  const docs = JSON.parse(raw);
  const strip = (u) => (typeof u === 'string' ? u.replace(/\/+$/, '') : u);
  for (const doc of docs) {
    doc.url = strip(doc.url);
    if (doc.id) doc.id = strip(doc.id);
  }

  await writeFile(file, JSON.stringify(docs), 'utf8');
  logger.info(`Removed trailing slashes from ${docs.length} MCP corpus URLs.`);
}

/**
 * Rewrite `/docs/columns.html` to `/docs/columns` in the built HTML.
 *
 * `build.format: 'file'` is what keeps every page on this site reachable without
 * a redirect: Cloudflare's `auto-trailing-slash` serves `/changelog` straight
 * from `changelog.html`, where `format: 'directory'` would 307 it to
 * `/changelog/`. The site's own pages are hand-written and already link
 * extensionless. Starlight, though, derives its links from `build.format`, so it
 * emits `href="/docs/columns.html"` — and Cloudflare 307s that back to the
 * extensionless URL. Every sidebar click paid a round trip, and `rel=canonical`
 * pointed at a URL that redirects.
 *
 * Starlight has no option for this, so the links are corrected after the fact.
 * Base.astro already strips `.html` the same way when it builds its canonical.
 *
 * ponytail: a regex over the emitted HTML, not an HTML parser. The pattern is
 * anchored to quoted root-relative attribute values, which is the only shape
 * Astro emits for internal links.
 */
export default function stripHtmlLinks() {
  return {
    name: 'qrate:strip-html-links',
    hooks: {
      'astro:build:done': async ({ dir, logger }) => {
        const root = new URL(dir).pathname.replace(/^\/([A-Za-z]:)/, '$1');

        const walk = async (p) => {
          const out = [];
          for (const e of await readdir(p, { withFileTypes: true })) {
            const full = join(p, e.name);
            if (e.isDirectory()) out.push(...(await walk(full)));
            else if (e.name.endsWith('.html')) out.push(full);
          }
          return out;
        };

        // `href="/a/b.html"` and the absolute form in canonical/og:url. The 404
        // page is exempt: Cloudflare's `not_found_handling` looks for that exact
        // filename, and nothing links to it anyway.
        const rel = /((?:href|content)=")((?:https:\/\/[^"]+)?\/[^"?#]*?)\.html((?:[?#][^"]*)?")/g;

        let changed = 0;
        for (const file of await walk(root)) {
          const src = await readFile(file, 'utf8');
          const out = src.replace(rel, (whole, head, path, tail) =>
            path.endsWith('/404') ? whole : `${head}${path}${tail}`
          );
          if (out !== src) {
            await writeFile(file, out, 'utf8');
            changed += 1;
          }
        }
        logger.info(`Stripped .html from internal links in ${changed} pages.`);

        await tidyMcpCorpus(root, logger);
      },
    },
  };
}
