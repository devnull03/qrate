import { defineCollection } from 'astro:content';
import { docsLoader } from '@astrojs/starlight/loaders';
import { docsSchema } from '@astrojs/starlight/schema';

// The collection lives at src/content/docs/, and everything inside it sits in a
// further docs/ directory so the pages come out under /docs. Starlight has no
// `prefix` option; a nested directory is the documented way to do this.
// Most of the tree is generated — see scripts/sync-docs.mjs.
export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
};
