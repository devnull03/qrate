import { getCollection, type CollectionEntry } from 'astro:content';

export type Guide = CollectionEntry<'guides'>;

// Drafts show in `bun run dev` so they can be previewed, never in a build.
export async function getGuides(): Promise<Guide[]> {
  const all = await getCollection('guides', (g) => import.meta.env.DEV || !g.data.draft);
  return all.sort(
    (a, b) => b.data.published.getTime() - a.data.published.getTime() || a.data.title.localeCompare(b.data.title)
  );
}

export const readingMinutes = (body = '') =>
  Math.max(1, Math.round(body.split(/\s+/).filter(Boolean).length / 220));

export const formatDate = (d: Date) =>
  d.toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC' });
