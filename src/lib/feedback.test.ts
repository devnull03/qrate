import { test, expect } from 'bun:test';
import { categories, submit, validate } from './feedback';

function form() {
  const data = new FormData();
  for (const [key, value] of Object.entries({
    id: 'd721d1ca-6213-4bda-8928-21ca37671d96', category: 'bug', summary: 'Example',
    description: 'Steps and expected result', consent: 'yes', 'cf-turnstile-response': 'test',
  })) data.set(key, value);
  return data;
}
const env = {
  LINEAR_API_KEY: 'test-only', TURNSTILE_SECRET_KEY: 'test-only',
  FEEDBACK_LIMITER: { limit: async () => ({ success: true }) },
};
const request = (data = form(), url = 'https://qrate.dvnl.work/api/feedback') => new Request(url, {
  method: 'POST', headers: { Origin: 'https://qrate.dvnl.work' }, body: data,
});

test('all six categories validate; unknown fields, category and missing consent fail', () => {
  for (const category of Object.keys(categories)) {
    const data = form(); data.set('category', category);
    expect(validate(data).category).toBe(category);
  }
  for (const [key, value] of [['category', '__proto__'], ['consent', ''], ['unexpected', 'x']]) {
    const data = form(); data.set(key, value);
    expect(() => validate(data)).toThrow();
  }
});

test('reject executable and oversized attachments', () => {
  const data = form();
  data.append('files', new File(['x'], 'secret.exe'));
  expect(() => validate(data)).toThrow();
  data.set('files', new File([new Uint8Array(5 * 1024 * 1024 + 1)], 'large.log'));
  expect(() => validate(data)).toThrow();
});

test('automatic compressed log does not consume a user attachment slot', () => {
  const data = form();
  for (let index = 0; index < 3; index++) {
    data.append('files', new File(['image'], `screenshot-${index}.png`));
  }
  data.append('files', new File(['gzip'], 'qrate-session.log.gz'));
  expect(validate(data).files).toHaveLength(4);
});

test('no provider calls for invalid input or unconfigured service', async () => {
  const never = async () => { throw new Error('Unexpected network call'); };
  expect((await submit(request(), {}, never as any)).status).toBe(503);
  const data = form(); data.delete('consent');
  expect((await submit(request(data), env, never as any)).status).toBe(400);
});

test('create issue with exact routing and triage labels', async () => {
  let input: any;
  const mocked = async (url: string, options: any) => {
    if (url.includes('siteverify')) return Response.json({ success: true, hostname: 'qrate.dvnl.work', action: 'feedback' });
    const body = JSON.parse(options.body);
    if (body.query.startsWith('query')) return Response.json({ data: { issues: { nodes: [] } } });
    input = body.variables.input;
    return Response.json({ data: { issueCreate: { success: true, issue: { id: input.id, identifier: 'TSGB-42' } } } });
  };
  const response = await submit(request(), env, mocked as any);
  expect(response.status).toBe(200);
  expect(await response.json()).toEqual({
    receipt: 'd721d1ca-6213-4bda-8928-21ca37671d96',
    ticket: 'TSGB-42',
  });
  expect(input.stateId).toBe('4b7df4af-a498-4656-8d0a-c87e1c1d28aa');
  expect(input.projectMilestoneId).toBe('7cdb86ec-c9a9-4d58-b46b-2e382566f1f5');
  expect(input.labelIds).toContain('88dcf4fa-6ccd-4d48-b427-d6d190745e05');
  expect(input.labelIds).not.toContain('0429eef3-39f3-4f67-a303-a9f985122a61');
});

test('Turnstile wrong hostname fails before Linear', async () => {
  const mocked = async () => Response.json({ success: true, hostname: 'attacker.example', action: 'feedback' });
  expect((await submit(request(), env, mocked as any)).status).toBe(403);
});

test('Cloudflare test keys work only on a local form', async () => {
  const mocked = async (_url: string, options: any) => {
    if (options.body instanceof URLSearchParams) {
      return Response.json({
        success: true,
        hostname: 'example.com',
        metadata: { result_with_testing_key: true },
      });
    }
    return Response.json({ data: { issues: { nodes: [{ id: 'existing' }] } } });
  };
  const local = request(form(), 'http://localhost:4321/api/feedback');
  local.headers.set('Origin', 'http://localhost:4321');
  expect((await submit(local, env, mocked as any)).status).toBe(200);
  expect((await submit(request(), env, mocked as any)).status).toBe(403);
});

test('existing issue returns receipt without creating another', async () => {
  let calls = 0;
  const mocked = async () => {
    calls++;
    return calls === 1
      ? Response.json({ success: true, hostname: 'qrate.dvnl.work', action: 'feedback' })
      : Response.json({ data: { issues: { nodes: [{ id: 'existing' }] } } });
  };
  expect((await submit(request(), env, mocked as any)).status).toBe(200);
  expect(calls).toBe(2);
});

test('GraphQL errors with HTTP 200 never report success', async () => {
  let calls = 0;
  const mocked = async () => ++calls === 1
    ? Response.json({ success: true, hostname: 'qrate.dvnl.work', action: 'feedback' })
    : Response.json({ errors: [{ message: 'private provider error' }] });
  const response = await submit(request(), env, mocked as any);
  expect(response.status).toBe(502);
  expect(await response.text()).not.toContain('private provider error');
});

test('log upload uses generic filename and labels only the successful issue', async () => {
  let input: any;
  let uploadName: string | undefined;
  const data = form();
  data.append('files', new File(['redacted log'], 'private-collection.log'));
  const mocked = async (url: string, options: any) => {
    if (url.includes('siteverify')) return Response.json({ success: true, hostname: 'qrate.dvnl.work', action: 'feedback' });
    if (options.method === 'PUT') return new Response(null, { status: 200 });
    const body = JSON.parse(options.body);
    if (body.query.startsWith('query')) return Response.json({ data: { issues: { nodes: [] } } });
    if (body.query.includes('fileUpload')) {
      uploadName = body.variables.name;
      return Response.json({ data: { fileUpload: { success: true, uploadFile: { uploadUrl: 'https://upload.example', assetUrl: 'https://asset.example/log', headers: [] } } } });
    }
    input = body.variables.input;
    return Response.json({ data: { issueCreate: { success: true, issue: { id: input.id } } } });
  };
  expect((await submit(request(data), env, mocked as any)).status).toBe(200);
  expect(uploadName).toBe('log-1.log');
  expect(input.labelIds).toContain('0429eef3-39f3-4f67-a303-a9f985122a61');
  expect(input.description).not.toContain('private-collection');
});

test('rate limit stops all provider work', async () => {
  const response = await submit(request(), {
    ...env, FEEDBACK_LIMITER: { limit: async () => ({ success: false }) },
  }, (() => { throw new Error('Unexpected fetch'); }) as any);
  expect(response.status).toBe(429);
});
