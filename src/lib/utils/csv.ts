/**
 * Browser-compatible CSV parser utility
 * Replaces the Node.js csv/sync package with a lightweight browser implementation
 */

export interface CSVParseOptions {
	delimiter?: string;
	quote?: string;
	escape?: string;
	skip_empty_lines?: boolean;
	trim?: boolean;
	columns?: boolean;
	cast?: boolean;
}

/**
 * Simple CSV parser for browser environments
 * @param csvData - Raw CSV string
 * @param options - Parsing options
 * @returns Parsed CSV data as 2D array
 */
export function parse(csvData: string, options: CSVParseOptions = {}): string[][] {
	const {
		delimiter = ',',
		quote = '"',
		escape = '"',
		skip_empty_lines = false,
		trim = false,
		columns = false,
		cast = false
	} = options;

	if (!csvData || typeof csvData !== 'string') {
		return [];
	}

	const result: string[][] = [];
	let currentRow: string[] = [];
	let currentField = '';
	let inQuotes = false;
	let i = 0;

	while (i < csvData.length) {
		const char = csvData[i];
		const nextChar = csvData[i + 1];

		if (char === quote) {
			if (inQuotes && nextChar === quote) {
				// Escaped quote
				currentField += quote;
				i += 2;
			} else {
				// Toggle quote state
				inQuotes = !inQuotes;
				i++;
			}
		} else if (char === delimiter && !inQuotes) {
			// Field separator
			if (trim) {
				currentField = currentField.trim();
			}
			currentRow.push(currentField);
			currentField = '';
			i++;
		} else if ((char === '\n' || char === '\r') && !inQuotes) {
			// Row separator
			if (trim) {
				currentField = currentField.trim();
			}
			currentRow.push(currentField);

			// Skip empty rows if requested
			if (!skip_empty_lines || currentRow.some(field => field.length > 0)) {
				result.push(currentRow);
			}

			currentRow = [];
			currentField = '';

			// Skip \r\n sequences
			if (char === '\r' && nextChar === '\n') {
				i += 2;
			} else {
				i++;
			}
		} else {
			// Regular character
			currentField += char;
			i++;
		}
	}

	// Handle last field/row
	if (currentField || currentRow.length > 0) {
		if (trim) {
			currentField = currentField.trim();
		}
		currentRow.push(currentField);

		if (!skip_empty_lines || currentRow.some(field => field.length > 0)) {
			result.push(currentRow);
		}
	}

	return result;
}

/**
 * Convert 2D array to CSV string
 * @param data - 2D array of data
 * @param options - Formatting options
 * @returns CSV string
 */
export function stringify(data: any[][], options: { delimiter?: string; quote?: string } = {}): string {
	const { delimiter = ',', quote = '"' } = options;

	return data.map(row =>
		row.map(field => {
			const stringField = String(field || '');

			// Quote field if it contains delimiter, quote, or newline
			if (stringField.includes(delimiter) || stringField.includes(quote) || stringField.includes('\n') || stringField.includes('\r')) {
				return quote + stringField.replace(new RegExp(quote, 'g'), quote + quote) + quote;
			}

			return stringField;
		}).join(delimiter)
	).join('\n');
}

/**
 * Parse CSV with object mode (first row as headers)
 * @param csvData - Raw CSV string
 * @param options - Parsing options
 * @returns Array of objects with headers as keys
 */
export function parseToObjects(csvData: string, options: CSVParseOptions = {}): Record<string, string>[] {
	const rows = parse(csvData, options);

	if (rows.length === 0) {
		return [];
	}

	const headers = rows[0];
	const dataRows = rows.slice(1);

	return dataRows.map(row => {
		const obj: Record<string, string> = {};
		headers.forEach((header, index) => {
			obj[header] = row[index] || '';
		});
		return obj;
	});
}

/**
 * Generate sample CSV data (replaces csv generate function)
 * @param options - Generation options
 * @returns Generated CSV data
 */
export function generateCSV(options: {
	columns?: string[];
	length?: number;
	delimiter?: string;
	objectMode?: boolean;
}): any[][] | Record<string, string>[] {
	const {
		columns = ['column1', 'column2', 'column3'],
		length = 10,
		delimiter = ',',
		objectMode = false
	} = options;

	const data: any[][] = [];

	// Add header row
	data.push([...columns]);

	// Generate data rows
	for (let i = 0; i < length; i++) {
		const row = columns.map((col, index) => `${col}_${i}_${index}`);
		data.push(row);
	}

	if (objectMode) {
		return data.slice(1).map(row => {
			const obj: Record<string, string> = {};
			columns.forEach((header, index) => {
				obj[header] = row[index] || '';
			});
			return obj;
		});
	}

	return data;
}