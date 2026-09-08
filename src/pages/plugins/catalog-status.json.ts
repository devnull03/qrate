import type { APIRoute } from 'astro';
import { catalogArtifactResponse } from '../../lib/catalog-response';

export const prerender = false;

export const GET: APIRoute = () => catalogArtifactResponse('status');
