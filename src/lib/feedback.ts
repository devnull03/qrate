export const categories: Record<string, string> = {
  bug: '9c1c54ef-3835-4b07-b7fe-c02952216f74',
  ui_ux: '8ac21715-4a1e-4a00-a4bb-06276a4ff7ca',
  feature: 'bf455f57-e866-46ae-8a73-5dcdd4f1cb23',
  crash: 'ed742751-05de-42db-a428-e52864a8a907',
  performance: '154aeee1-f247-4bc6-b797-3b06051f1a7b',
  improvement: '486d0f47-7520-42d5-88a5-c1fde5dcf769',
};
export const platforms: Record<string, string> = {
  windows: '951df0ef-4d13-4eaa-97cc-b7621702126c',
  macos: '6ad3615e-c1ad-4be6-91bf-9ce6a9d6b093',
  linux: '2dae9fe2-a98c-43bd-90d5-3a786923d8af',
};
export const MAX_BYTES = 17 * 1024 * 1024;

export function validate(form: FormData) {
  const allowed = ['id', 'category', 'platform', 'summary', 'description', 'email', 'diagnostics', 'logs', 'consent', 'cf-turnstile-response', 'files'];
  for (const key of form.keys()) {
    if (!allowed.includes(key) || (key !== 'files' && form.getAll(key).length !== 1)) throw new Error('Invalid fields');
  }
  const text = (key: string, max: number, required = false) => {
    const value = form.get(key) ?? '';
    if (typeof value !== 'string' || value.length > max || (required && !value.trim())) throw new Error(`Invalid ${key}`);
    return value.trim();
  };
  const id = text('id', 36, true);
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(id)) throw new Error('Invalid ID');
  const category = text('category', 20, true);
  if (!Object.hasOwn(categories, category)) throw new Error('Invalid category');
  const platform = text('platform', 20);
  if (platform && !Object.hasOwn(platforms, platform)) throw new Error('Invalid platform');
  if (text('consent', 3) !== 'yes') throw new Error('Review required');
  const email = text('email', 254);
  if (email && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) throw new Error('Invalid email');
  const fileValues = form.getAll('files');
  if (fileValues.some(file => typeof file === 'string' && file !== '')) throw new Error('Invalid files');
  const files = fileValues.filter((file): file is File =>
    typeof file !== 'string' && (file.size > 0 || file.name.length > 0));
  const automaticLogs = files.filter(file => /^qrate-session\.log(?:\.gz)?$/i.test(file.name));
  const attachments = files.filter(file => !automaticLogs.includes(file));
  if (automaticLogs.length > 1) throw new Error('Only one automatic session log is allowed.');
  if (attachments.length > 3) throw new Error('Choose no more than three attachments.');
  for (const file of files) {
    const automatic = automaticLogs.includes(file);
    const sizeLimit = automatic ? 512 * 1024 : 5 * 1024 * 1024;
    if (file.size > sizeLimit || !/\.(png|jpg|jpeg|txt|log|log\.gz)$/i.test(file.name)) {
      throw new Error(automatic
        ? 'The automatic session log is too large.'
        : 'Use PNG, JPEG, TXT or LOG attachments up to 5 MiB each.');
    }
  }
  return {
    id, category, platform, email, files,
    summary: text('summary', 160, true),
    description: text('description', 10000, true),
    diagnostics: text('diagnostics', 8000),
    logs: text('logs', 64 * 1024),
    token: text('cf-turnstile-response', 2048, true),
  };
}

export async function submit(request: Request, env: any, fetcher: typeof fetch = fetch): Promise<Response> {
  const reply = (status: number, body: object) => Response.json(body, { status, headers: { 'Cache-Control': 'no-store' } });
  if (!env.LINEAR_API_KEY || !env.TURNSTILE_SECRET_KEY || !env.FEEDBACK_LIMITER) return reply(503, { error: 'Feedback is not configured yet.' });
  if (request.headers.get('origin') !== new URL(request.url).origin) return reply(403, { error: 'Open the qrate feedback form to submit.' });
  let report: ReturnType<typeof validate>;
  try {
    if (!request.headers.get('content-type')?.startsWith('multipart/form-data')) throw new Error('Expected form');
    const reader = request.body?.getReader();
    if (!reader) throw new Error('Missing body');
    const chunks: Uint8Array[] = [];
    let size = 0;
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.length;
      if (size > MAX_BYTES) { await reader.cancel(); return reply(413, { error: 'Report is too large.' }); }
      chunks.push(value);
    }
    const form = await new Response(new Blob(chunks), { headers: { 'content-type': request.headers.get('content-type')! } }).formData();
    try {
      report = validate(form);
    } catch (error) {
      return reply(400, { error: error instanceof Error ? error.message : 'Check the report fields.' });
    }
  } catch {
    return reply(400, { error: 'The report could not be read. Please retry.' });
  }
  try {
    const verification = await fetcher('https://challenges.cloudflare.com/turnstile/v0/siteverify', {
      method: 'POST', body: new URLSearchParams({ secret: env.TURNSTILE_SECRET_KEY, response: report.token }),
      signal: AbortSignal.timeout(15000),
    });
    const challenge = await verification.json() as any;
    const requestHost = new URL(request.url).hostname;
    const localTest = ['localhost', '127.0.0.1'].includes(requestHost)
      && challenge.metadata?.result_with_testing_key === true;
    const production = challenge.hostname === requestHost && challenge.action === 'feedback';
    if (!verification.ok || !challenge.success || (!localTest && !production)) {
      return reply(403, { error: 'Verification expired. Please try again.' });
    }
    const { success } = await env.FEEDBACK_LIMITER.limit({ key: request.headers.get('cf-connecting-ip') || 'unknown' });
    if (!success) return reply(429, { error: 'Too many requests. Please try again later.' });
    const graphql = async (query: string, variables: object) => {
      const response = await fetcher('https://api.linear.app/graphql', {
        method: 'POST',
        headers: { Authorization: env.LINEAR_API_KEY, 'Content-Type': 'application/json' },
        body: JSON.stringify({ query, variables }),
        signal: AbortSignal.timeout(20000),
      });
      const result = await response.json() as any;
      if (!response.ok || result.errors?.length || !result.data) throw new Error('Linear failed');
      return result.data;
    };
    // A stable issue UUID makes retries safe without a second database.
    const existing = await graphql('query($id: ID!) { issues(filter: { id: { eq: $id }, project: { id: { eq: "aa8cfc24-95f6-461e-aac4-46437d89459e" } } }) { nodes { id identifier } } }', { id: report.id });
    if (existing.issues.nodes.length) return reply(200, { receipt: report.id, ticket: existing.issues.nodes[0].identifier });
    const attachments: string[] = [];
    let logs = Boolean(report.logs);
    for (const [index, file] of report.files.entries()) {
      const extension = file.name.split('.').pop()!.toLowerCase();
      const isLog = ['txt', 'log', 'gz'].includes(extension);
      const type = extension === 'gz' ? 'application/gzip' : isLog ? 'text/plain' : extension === 'png' ? 'image/png' : 'image/jpeg';
      const filename = `${isLog ? 'log' : 'screenshot'}-${index + 1}.${extension}`;
      const upload = await graphql(
        'mutation($type: String!, $name: String!, $size: Int!) { fileUpload(contentType: $type, filename: $name, size: $size) { success uploadFile { uploadUrl assetUrl headers { key value } } } }',
        { type, name: filename, size: file.size },
      );
      if (!upload.fileUpload.success || !upload.fileUpload.uploadFile) throw new Error('Upload failed');
      const target = upload.fileUpload.uploadFile;
      const headers = new Headers({ 'Content-Type': type });
      for (const { key, value } of target.headers) headers.set(key, value);
      const stored = await fetcher(target.uploadUrl, { method: 'PUT', headers, body: file, signal: AbortSignal.timeout(30000) });
      if (!stored.ok) throw new Error('Upload failed');
      attachments.push(`[${filename}](${target.assetUrl})`);
      logs ||= isLog;
    }
    const labelIds = [
      categories[report.category],
      ...(report.platform ? [platforms[report.platform]] : []),
      ...(logs ? ['0429eef3-39f3-4f67-a303-a9f985122a61'] : []),
    ];
    const created = await graphql('mutation($input: IssueCreateInput!) { issueCreate(input: $input) { success issue { id identifier } } }', {
      input: {
        id: report.id, teamId: 'd14f1e07-93ce-4cfe-968c-c6372c6f58a6',
        projectId: 'aa8cfc24-95f6-461e-aac4-46437d89459e',
        projectMilestoneId: '7cdb86ec-c9a9-4d58-b46b-2e382566f1f5',
        stateId: '4b7df4af-a498-4656-8d0a-c87e1c1d28aa', labelIds,
        title: report.summary,
        description: [
          report.description,
          report.email ? `Reply to: ${report.email}` : '',
          report.diagnostics || report.logs || attachments.length ? '---' : '',
          report.diagnostics
            ? `## User-reviewed diagnostics\n\n\`\`\`json\n${report.diagnostics}\n\`\`\``
            : '',
          report.logs
            ? `## User-reviewed application log\n\n\`\`\`text\n${report.logs}\n\`\`\``
            : '',
          ...attachments,
        ].filter(Boolean).join('\n\n'),
      },
    });
    if (!created.issueCreate.success || !created.issueCreate.issue) throw new Error('Create failed');
    return reply(200, { receipt: report.id, ticket: created.issueCreate.issue.identifier });
  } catch {
    return reply(502, { error: 'Delivery could not be confirmed. Your form is unchanged. Retry with the same report ID.' });
  }
}
