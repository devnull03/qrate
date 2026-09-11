import { expect, test } from 'bun:test';
import { fileURLToPath } from 'node:url';
import { getPluginCatalog } from './plugins';

test('development fixture has a valid production signature', async () => {
  process.env.QRATE_PLUGIN_CATALOG_FILE = fileURLToPath(
    new URL('../../fixtures/plugin-catalog/catalog.json', import.meta.url),
  );
  const catalog = await getPluginCatalog();
  expect(catalog.plugins.map((plugin) => plugin.id)).toEqual(['org.islandora.vocabularies']);
});
