import { env } from 'cloudflare:workers';

type CatalogArtifact = 'json' | 'signature' | 'status';
type CatalogSchema = 'catalog' | 'listing' | 'package';

const artifactTypes: Record<CatalogArtifact, string> = {
  json: 'application/json; charset=utf-8',
  signature: 'application/json; charset=utf-8',
  status: 'application/json; charset=utf-8',
};

export async function catalogArtifactResponse(artifact: CatalogArtifact): Promise<Response> {
  const version = await env.PLUGIN_CATALOG.get('current');
  if (!version) return unavailable();

  const body = await env.PLUGIN_CATALOG.get(`catalog:${version}:${artifact}`);
  if (!body) return unavailable();

  return new Response(body, {
    headers: {
      'cache-control': 'public, max-age=300',
      'content-type': artifactTypes[artifact],
      etag: `"${version}"`,
    },
  });
}

export async function catalogSchemaResponse(schema: string): Promise<Response> {
  const name = schema.endsWith('.schema') ? schema.slice(0, -'.schema'.length) : schema;
  if (!(['catalog', 'listing', 'package'] as string[]).includes(name)) {
    return new Response('Not found\n', { status: 404 });
  }
  const body = await env.PLUGIN_CATALOG.get(`schema:${name as CatalogSchema}`);
  if (!body) return unavailable();

  return new Response(body, {
    headers: {
      'cache-control': 'public, max-age=86400',
      'content-type': 'application/schema+json; charset=utf-8',
    },
  });
}

function unavailable(): Response {
  return new Response('Plugin catalog is not available\n', {
    status: 503,
    headers: {
      'cache-control': 'no-store',
      'content-type': 'text/plain; charset=utf-8',
    },
  });
}
