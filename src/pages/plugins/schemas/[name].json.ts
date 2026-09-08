import type { APIRoute } from 'astro';
import { catalogSchemaResponse } from '../../../lib/catalog-response';

export const prerender = false;

export const GET: APIRoute = ({ params }) => catalogSchemaResponse(params.name ?? '');
