// The non-DOM half of /convert: what files are called, what errors say, and the
// two Google calls. The page script owns the DOM; the WASM module owns the
// formats. See qrate-export-spec.md for the module's interface.

export type Format = 'csv' | 'xlsx' | 'jsonld' | 'csl';

const EXT: Record<Format, string> = {
  csv: 'csv',
  xlsx: 'xlsx',
  jsonld: 'jsonld',
  csl: 'json',
};

export const MIME: Record<Format, string> = {
  csv: 'text/csv',
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  jsonld: 'application/ld+json',
  csl: 'application/vnd.citationstyles.csl+json',
};

/** The project's own name when it has one, else the file's, made safe for every OS. */
export function baseName(projectName: string | undefined, fileName: string): string {
  const raw = projectName?.trim() || fileName.replace(/\.qrate$/i, '');
  return raw.replace(/[\\/:*?"<>|\x00-\x1f]+/g, '-').replace(/^[\s.]+|[\s.]+$/g, '') || 'export';
}

export function outputName(base: string, format: Format): string {
  return format === 'csl' ? `${base}.csl.json` : `${base}.${EXT[format]}`;
}

/** Past this, reading the whole file into memory is worth a warning first. */
export const LARGE_FILE = 500 * 1024 * 1024;

/**
 * The module's errors read `code: detail`. The page shows its own sentence for
 * the code; the detail only goes to the console.
 */
export function openErrorMessage(error: unknown): string {
  const code = String(error instanceof Error ? error.message : error).split(':')[0];
  switch (code) {
    case 'not-qrate':
      return "This isn't a qrate project. Look for a file that ends in .qrate.";
    case 'newer':
      return 'This project was saved by a newer qrate than this page supports. Reload the page, or export it from the app.';
    case 'empty':
      return 'This project has no rows to export.';
    default:
      return "This project couldn't be read. It may be damaged, or still being written by qrate.";
  }
}

export const LOAD_FAILED =
  "Your browser couldn't load the converter. Try a current version of Chrome, Firefox, Safari or Edge.";

// --- Google, mirroring data-exchange/src/google.rs ---------------------------

export const SHEETS_SCOPE = 'https://www.googleapis.com/auth/drive.file';

export class NoAccess extends Error {}

async function sheets(token: string, url: string, init: RequestInit): Promise<any> {
  const response = await fetch(url, {
    ...init,
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
  });
  // drive.file answers 404, not 403, for a file the token was never granted.
  if (response.status === 404) throw new NoAccess();
  if (!response.ok) throw new Error(`Google answered ${response.status}`);
  return response.json();
}

export async function createSheet(token: string, title: string): Promise<string> {
  const created = await sheets(token, 'https://sheets.googleapis.com/v4/spreadsheets', {
    method: 'POST',
    body: JSON.stringify({ properties: { title } }),
  });
  return created.spreadsheetId;
}

/** Fills the first tab. "A1" with no sheet name is the first tab, whose title is localised. */
export async function writeValues(token: string, id: string, values: string[][]): Promise<void> {
  await sheets(
    token,
    `https://sheets.googleapis.com/v4/spreadsheets/${encodeURIComponent(id)}/values/A1?valueInputOption=RAW`,
    { method: 'PUT', body: JSON.stringify({ values }) },
  );
}

/** Applies project notes to the same first tab as the values. */
export async function writeNotes(
  token: string,
  id: string,
  requestsForSheet: (sheetId: number) => { requests: unknown[] } | undefined,
): Promise<void> {
  const url = `https://sheets.googleapis.com/v4/spreadsheets/${encodeURIComponent(id)}`;
  const spreadsheet = await sheets(token, `${url}?fields=sheets(properties(sheetId))`, { method: 'GET' });
  const sheetId = spreadsheet.sheets?.[0]?.properties?.sheetId;
  if (!Number.isInteger(sheetId)) throw new Error('Google returned a spreadsheet with no tab');
  const body = requestsForSheet(sheetId);
  if (body?.requests.length) {
    await sheets(token, `${url}:batchUpdate`, { method: 'POST', body: JSON.stringify(body) });
  }
}

export const sheetUrl = (id: string) => `https://docs.google.com/spreadsheets/d/${id}`;

export function googleErrorMessage(error: unknown): string {
  if (error instanceof NoAccess) {
    return 'qrate can only write to sheets you pick in Google’s file chooser. Pick the sheet again.';
  }
  const type = (error as { type?: string })?.type;
  if (type === 'popup_failed_to_open') return 'Allow pop-ups for this site, then try again.';
  const status = /Google answered (\d+)/.exec(String((error as Error)?.message))?.[1];
  if (status === '429') return 'Google is limiting requests right now. Wait a minute and try again.';
  if (status === '400') return 'Google refused the data. The project may be larger than a sheet allows.';
  return 'Google Sheets could not be reached. Check your connection and try again.';
}
