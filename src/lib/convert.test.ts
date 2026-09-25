import { test, expect } from 'bun:test';
import { NoAccess, baseName, googleErrorMessage, openErrorMessage, outputName, writeNotes } from './convert';

test('names come from the project, fall back to the file, and are safe on every OS', () => {
  expect(baseName('Fonds 12: letters/photos', 'x.qrate')).toBe('Fonds 12- letters-photos');
  expect(baseName('  ', 'Catalogue.QRATE')).toBe('Catalogue');
  expect(baseName(undefined, '...qrate')).toBe('export');
  expect(outputName('Fonds', 'csl')).toBe('Fonds.csl.json');
  expect(outputName('Fonds', 'xlsx')).toBe('Fonds.xlsx');
});

test('module error codes map to the page’s own sentences', () => {
  expect(openErrorMessage(new Error('not-qrate: application_id does not match'))).toContain("isn't a qrate project");
  expect(openErrorMessage(new Error('newer: saved by a newer qrate'))).toContain('newer qrate');
  expect(openErrorMessage(new Error('empty: no rows'))).toContain('no rows');
  expect(openErrorMessage('something else')).toContain("couldn't be read");
});

test('Google failures read as what to do next', () => {
  expect(googleErrorMessage(new NoAccess())).toContain('Pick the sheet again');
  expect(googleErrorMessage({ type: 'popup_failed_to_open' })).toContain('pop-ups');
  expect(googleErrorMessage(new Error('Google answered 429'))).toContain('Wait a minute');
});

test('Sheet notes use the first tab ID and leave values alone', async () => {
  const originalFetch = globalThis.fetch;
  const calls: Array<{ url: string; init: RequestInit }> = [];
  globalThis.fetch = async (input, init) => {
    calls.push({ url: String(input), init: init! });
    return Response.json(calls.length === 1 ? { sheets: [{ properties: { sheetId: 73 } }] } : {});
  };
  try {
    await writeNotes('token', 'a/b', (sheetId) => ({
      requests: [{ updateCells: { start: { sheetId, rowIndex: 2, columnIndex: 1 }, fields: 'note' } }],
    }));
    expect(calls[0].url).toContain('a%2Fb?fields=sheets(properties(sheetId))');
    expect(calls[1].url).toContain('a%2Fb:batchUpdate');
    expect(JSON.parse(calls[1].init.body as string).requests[0].updateCells.start.sheetId).toBe(73);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
