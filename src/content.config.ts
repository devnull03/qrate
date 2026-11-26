import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { z } from 'astro/zod';
import { docsLoader } from '@astrojs/starlight/loaders';
import { docsSchema } from '@astrojs/starlight/schema';

// The collection lives at src/content/docs/, and everything inside it sits in a
// further docs/ directory so the pages come out under /docs. Starlight has no
// `prefix` option; a nested directory is the documented way to do this.
// Most of the tree is generated — see scripts/sync-docs.mjs.
//
// Guides are written here on the site branch, not synced from main: they walk
// through a task end to end, where the docs describe one feature at a time.
export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
  guides: defineCollection({
    loader: glob({ pattern: '*.{md,mdx}', base: './src/content/guides' }),
    schema: z.object({
      title: z.string(),
      description: z.string(),
      qrateVersion: z.string().regex(/^\d+\.\d+\.\d+/),
      series: z.enum(['start-here', 'guides']).default('guides'),
      order: z.number().default(100),
      published: z.coerce.date(),
      updated: z.coerce.date().optional(),
      tags: z.array(z.string()).default([]),
      related: z.array(z.object({ label: z.string(), href: z.string() })).default([]),
      draft: z.boolean().default(false),
    }),
  }),
};
