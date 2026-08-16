import { defineRouteMiddleware } from '@astrojs/starlight/route-data';
import synced from './content/docs/docs/.synced.json';

const REPO = 'https://github.com/devnull03/qrate';

// Written by scripts/sync-docs.mjs: the pages that are copies of main:docs/,
// relative to the collection's docs/ directory.
const generated = new Set(synced as string[]);

/**
 * Two corrections Starlight has no config option for.
 *
 * 1. Tab title order. The marketing pages read `qrate · changelog and release
 *    notes` — name, middot, subject. Starlight builds `Columns | qrate` in
 *    `getHead` and offers only the delimiter, not the order.
 *
 * 2. Edit links. Most of these pages are generated on the `site` branch from
 *    `main:docs/`, so the default edit URL points at a file that the next
 *    `bun run sync-docs` overwrites. Send those to the real source instead;
 *    hand-authored pages keep pointing at this branch.
 */
export const onRequest = defineRouteMiddleware((context) => {
  const route = context.locals.starlightRoute;

  const title = route.head.find((tag) => tag.tag === 'title');
  if (title) title.content = `qrate docs · ${route.entry.data.title}`;

  const rel = route.entry.filePath?.replace(/^src\/content\/docs\/docs\//, '');
  if (rel && generated.has(rel)) {
    route.editUrl = new URL(`${REPO}/edit/main/docs/${rel}`);
  }
});
