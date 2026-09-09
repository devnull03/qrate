import type { APIRoute } from 'astro';
import { env } from 'cloudflare:workers';
import { submit } from '../../lib/feedback';

export const prerender = false;
export const POST: APIRoute = ({ request }) => submit(request, env);
