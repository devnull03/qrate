import { parse } from './csv';

/**
 * Interface for parsed CSV data
 */
export interface ParsedSheetData {
	headers: string[];
	rows: Record<string, string>[];
	rawData: string[][];
}

/**
 * Options for parsing CSV data
 */
export interface ParseOptions {
	skipEmptyLines?: boolean;
	trimFields?: boolean;
	delimiter?: string;
	quote?: string;
	escape?: string;
}

/**
 * Extracts the sheet ID from a Google Sheets URL
 * @param url - The Google Sheets URL
 * @returns The sheet ID or null if not found
 */
export function extractSheetId(url: string): string | null {
	try {
		// Handle various Google Sheets URL formats
		const patterns = [
			// Standard sharing URL: https://docs.google.com/spreadsheets/d/{ID}/edit
			/\/spreadsheets\/d\/([a-zA-Z0-9-_]+)/,
			// Direct CSV export URL: https://docs.google.com/spreadsheets/d/{ID}/export?format=csv
			/\/spreadsheets\/d\/([a-zA-Z0-9-_]+)\/export/,
			// Legacy format
			/spreadsheets\/d\/([a-zA-Z0-9-_]+)/
		];

		for (const pattern of patterns) {
			const match = url.match(pattern);
			if (match && match[1]) {
				return match[1];
			}
		}

		return null;
	} catch (error) {
		console.error('Error extracting sheet ID:', error);
		return null;
	}
}

/**
 * Converts a Google Sheets URL to a CSV export URL
 * @param url - The original Google Sheets URL
 * @param gid - Optional sheet GID (for specific tabs)
 * @returns The CSV export URL or null if invalid
 */
export function convertToCSVUrl(url: string, gid?: string): string | null {
	const sheetId = extractSheetId(url);
	if (!sheetId) {
		return null;
	}

	let csvUrl = `https://docs.google.com/spreadsheets/d/${sheetId}/export?format=csv`;

	// Add specific sheet tab if GID is provided
	if (gid) {
		csvUrl += `&gid=${gid}`;
	}

	return csvUrl;
}

/**
 * Validates if a URL is a valid Google Sheets URL
 * @param url - The URL to validate
 * @returns True if valid Google Sheets URL
 */
export function isValidGoogleSheetsUrl(url: string): boolean {
	try {
		const urlObj = new URL(url);
		return (
			urlObj.hostname === 'docs.google.com' &&
			urlObj.pathname.includes('/spreadsheets/') &&
			extractSheetId(url) !== null
		);
	} catch {
		return false;
	}
}

/**
 * Fetches raw CSV data from a Google Sheets URL
 * @param url - The Google Sheets URL
 * @param gid - Optional sheet GID for specific tabs
 * @returns Promise resolving to raw CSV string
 */
export async function fetchRawCSV(url: string, gid?: string): Promise<string> {
	if (!isValidGoogleSheetsUrl(url)) {
		throw new Error('Invalid Google Sheets URL provided');
	}

	const csvUrl = convertToCSVUrl(url, gid);
	if (!csvUrl) {
		throw new Error('Failed to convert URL to CSV format');
	}

	try {
		const response = await fetch(csvUrl);

		if (!response.ok) {
			throw new Error(`HTTP error! status: ${response.status} - ${response.statusText}`);
		}

		const csvData = await response.text();

		if (!csvData || csvData.trim().length === 0) {
			throw new Error('Received empty CSV data');
		}

		return csvData;
	} catch (error) {
		if (error instanceof Error) {
			throw new Error(`Failed to fetch Google Sheets data: ${error.message}`);
		}
		throw new Error('Unknown error occurred while fetching Google Sheets data');
	}
}

/**
 * Parses CSV string into structured data using the csv module
 * @param csvData - Raw CSV string
 * @param options - Parsing options
 * @returns Parsed sheet data
 */
export function parseCSVData(csvData: string, options: ParseOptions = {}): ParsedSheetData {
	const defaultOptions: ParseOptions = {
		skipEmptyLines: true,
		trimFields: true,
		delimiter: ',',
		quote: '"',
		escape: '"'
	};

	const parseOptions = { ...defaultOptions, ...options };

	try {
		// Parse CSV using the csv module
		const rawData: string[][] = parse(csvData, {
			skip_empty_lines: parseOptions.skipEmptyLines,
			trim: parseOptions.trimFields,
			delimiter: parseOptions.delimiter,
			quote: parseOptions.quote,
			escape: parseOptions.escape,
			columns: false, // We'll handle headers manually for more control
			cast: false // Keep everything as strings
		});

		if (rawData.length === 0) {
			throw new Error('No data found in CSV');
		}

		// Extract headers (first row)
		const headers = rawData[0].map(header => header?.toString().trim() || '');

		// Convert rows to objects with headers as keys
		const rows: Record<string, string>[] = rawData.slice(1).map(row => {
			const rowObject: Record<string, string> = {};
			headers.forEach((header, index) => {
				rowObject[header] = row[index]?.toString().trim() || '';
			});
			return rowObject;
		});

		return {
			headers,
			rows,
			rawData
		};
	} catch (error) {
		if (error instanceof Error) {
			throw new Error(`Failed to parse CSV data: ${error.message}`);
		}
		throw new Error('Unknown error occurred while parsing CSV data');
	}
}

/**
 * Filters parsed sheet data based on criteria
 * @param data - Parsed sheet data
 * @param filterFn - Function to filter rows
 * @returns Filtered sheet data
 */
export function filterSheetData(
	data: ParsedSheetData,
	filterFn: (row: Record<string, string>) => boolean
): ParsedSheetData {
	const filteredRows = data.rows.filter(filterFn);
	const filteredRawData = [data.rawData[0]]; // Keep headers

	// Rebuild raw data for filtered rows
	filteredRows.forEach(row => {
		const rawRow = data.headers.map(header => row[header] || '');
		filteredRawData.push(rawRow);
	});

	return {
		headers: data.headers,
		rows: filteredRows,
		rawData: filteredRawData
	};
}

/**
 * Main function to fetch and parse Google Sheets data
 * @param link - Google Sheets URL
 * @param gid - Optional sheet GID for specific tabs
 * @param parseOptions - Optional parsing configuration
 * @returns Promise resolving to parsed sheet data
 */
export async function fetchFromGoogleSheets(
	link: string,
	gid?: string,
	parseOptions?: ParseOptions
): Promise<ParsedSheetData> {
	try {
		// Validate URL
		if (!link || typeof link !== 'string') {
			throw new Error('Valid Google Sheets URL is required');
		}

		// Fetch raw CSV data
		const csvData = await fetchRawCSV(link, gid);

		// Parse CSV data
		const parsedData = parseCSVData(csvData, parseOptions);

		// Validate parsed data
		if (parsedData.headers.length === 0) {
			throw new Error('No headers found in the spreadsheet');
		}

		console.log(`Successfully fetched and parsed Google Sheets data:`, {
			headers: parsedData.headers,
			rowCount: parsedData.rows.length,
			hasData: parsedData.rows.length > 0
		});

		return parsedData;
	} catch (error) {
		console.error('Error in fetchFromGoogleSheets:', error);

		if (error instanceof Error) {
			throw error;
		}
		throw new Error('Unknown error occurred while fetching Google Sheets data');
	}
}

/**
 * Utility function to convert parsed data back to CSV string
 * @param data - Parsed sheet data
 * @returns CSV string
 */
export function dataToCSV(data: ParsedSheetData): string {
	const csvRows = [
		data.headers.join(','),
		...data.rows.map(row =>
			data.headers.map(header => {
				const value = row[header] || '';
				// Escape quotes and wrap in quotes if contains comma or quote
				if (value.includes(',') || value.includes('"') || value.includes('\n')) {
					return `"${value.replace(/"/g, '""')}"`;
				}
				return value;
			}).join(',')
		)
	];

	return csvRows.join('\n');
}

/**
 * Utility function to get column data as array
 * @param data - Parsed sheet data
 * @param columnName - Name of the column
 * @returns Array of values from the specified column
 */
export function getColumnData(data: ParsedSheetData, columnName: string): string[] {
	if (!data.headers.includes(columnName)) {
		throw new Error(`Column "${columnName}" not found in headers`);
	}

	return data.rows.map(row => row[columnName] || '');
}

/**
 * Utility function to find rows by column value
 * @param data - Parsed sheet data
 * @param columnName - Name of the column to search
 * @param value - Value to search for
 * @param exactMatch - Whether to use exact match (default: true)
 * @returns Array of matching rows
 */
export function findRowsByColumnValue(
	data: ParsedSheetData,
	columnName: string,
	value: string,
	exactMatch: boolean = true
): Record<string, string>[] {
	if (!data.headers.includes(columnName)) {
		throw new Error(`Column "${columnName}" not found in headers`);
	}

	return data.rows.filter(row => {
		const cellValue = row[columnName] || '';
		return exactMatch
			? cellValue === value
			: cellValue.toLowerCase().includes(value.toLowerCase());
	});
}
