// Core spreadsheet utilities adapted from svelte-sheets for browser compatibility
// This extracts the essential functionality without Node.js dependencies

export interface SpreadsheetConfig {
	defaultColWidth: string;
	defaultRowHeight: string;
	minDimensions: [number, number];
	tableHeight?: string;
	tableWidth?: string;
	columnResize: boolean;
	rowResize: boolean;
	columnDrag: boolean;
	rowDrag: boolean;
	defaultColAlign: string;
	fullscreen?: boolean;
	toolbar?: boolean;
	tableOverflow?: boolean;
}

export const defaultConfig: SpreadsheetConfig = {
	defaultColWidth: "100px",
	defaultRowHeight: "24px",
	minDimensions: [10, 10],
	columnResize: true,
	rowResize: true,
	columnDrag: false,
	rowDrag: false,
	defaultColAlign: "left"
};

// Cell address encoding/decoding utilities
export function encodeCell(coord: { c: number; r: number }): string {
	return `${encodeCol(coord.c)}${coord.r + 1}`;
}

export function decodeCell(address: string): { c: number; r: number } {
	const match = address.match(/^([A-Z]+)(\d+)$/);
	if (!match) return { c: 0, r: 0 };

	const colStr = match[1];
	const rowStr = match[2];

	let c = 0;
	for (let i = 0; i < colStr.length; i++) {
		c = c * 26 + (colStr.charCodeAt(i) - 64);
	}
	c -= 1; // Convert to 0-based

	const r = parseInt(rowStr) - 1; // Convert to 0-based

	return { c, r };
}

export function encodeCol(col: number): string {
	let result = '';
	while (col >= 0) {
		result = String.fromCharCode(65 + (col % 26)) + result;
		col = Math.floor(col / 26) - 1;
	}
	return result;
}

// Selection utilities
export function getBorder(selection: [string, string]): {
	tl: { c: number; r: number };
	br: { c: number; r: number };
} {
	const [start, end] = selection.map(decodeCell);

	return {
		tl: {
			c: Math.min(start.c, end.c),
			r: Math.min(start.r, end.r)
		},
		br: {
			c: Math.max(start.c, end.c),
			r: Math.max(start.r, end.r)
		}
	};
}

// Data manipulation utilities
export function clearSelection(data: any[][], selected: [string, string]): any[][] {
	if (!selected) return data;

	const { tl, br } = getBorder(selected);
	const newData = [...data];

	for (let r = tl.r; r <= br.r; r++) {
		if (!newData[r]) continue;
		newData[r] = [...newData[r]];
		for (let c = tl.c; c <= br.c; c++) {
			newData[r][c] = '';
		}
	}

	return newData;
}

export function pasteSelection(
	data: any[][],
	clipboard: [string, string] | undefined,
	selected: [string, string]
): any[][] {
	if (!clipboard || !selected) return data;

	const clipboardBorder = getBorder(clipboard);
	const selectedBorder = getBorder(selected);

	const clipboardData: any[][] = [];
	for (let r = clipboardBorder.tl.r; r <= clipboardBorder.br.r; r++) {
		const row: any[] = [];
		for (let c = clipboardBorder.tl.c; c <= clipboardBorder.br.c; c++) {
			row.push(data[r]?.[c] || '');
		}
		clipboardData.push(row);
	}

	const newData = [...data];
	let clipboardRow = 0;

	for (let r = selectedBorder.tl.r; r <= selectedBorder.br.r; r++) {
		if (!newData[r]) {
			newData[r] = [];
		}
		newData[r] = [...newData[r]];

		let clipboardCol = 0;
		for (let c = selectedBorder.tl.c; c <= selectedBorder.br.c; c++) {
			const sourceValue = clipboardData[clipboardRow % clipboardData.length]?.[clipboardCol % clipboardData[0].length];
			newData[r][c] = sourceValue || '';
			clipboardCol++;
		}
		clipboardRow++;
	}

	return newData;
}

// Style computation utility
export function computeStyles(
	col: number,
	row: number,
	rowData: any,
	style: Record<string, string>,
	config: SpreadsheetConfig,
	currentValue?: any,
	nextValue?: any
): string {
	const cellAddress = encodeCell({ c: col, r: row });
	const cellStyle = style[cellAddress] || '';

	const overflow = nextValue && String(nextValue).length ? 'hidden' : 'visible';

	return `overflow: ${overflow}; ${cellStyle}`;
}

// Column and row sizing utilities
export function getColumnWidth(columnIndex: number, columns: any[]): number {
	const column = columns[columnIndex];
	if (!column) return 100;

	const width = column.width;
	if (typeof width === 'string') {
		return Number(width.replace('px', '')) || 100;
	}
	return Number(width) || 100;
}

export function getRowHeight(rowIndex: number, rows: any[]): number {
	const row = rows[rowIndex];
	if (!row) return 24;

	const height = row.height;
	if (typeof height === 'string') {
		return Math.max(Number(height.replace('px', '')) || 24, 24);
	}
	return Math.max(Number(height) || 24, 24);
}

// Virtual scrolling utilities
export function calculateVisibleRange(
	scrollPosition: number,
	viewportSize: number,
	itemSizes: number[],
	buffer: number = 5
): { start: number; end: number; offset: number } {
	let start = 0;
	let offset = 0;
	let cumulativeSize = 0;

	// Find start index
	for (let i = 0; i < itemSizes.length; i++) {
		if (cumulativeSize + itemSizes[i] > scrollPosition) {
			start = Math.max(0, i - buffer);
			offset = cumulativeSize - (i - start) * (itemSizes[i] || 24);
			break;
		}
		cumulativeSize += itemSizes[i] || 24;
	}

	// Find end index
	let end = start;
	let remainingViewport = viewportSize + buffer * 24;
	while (end < itemSizes.length && remainingViewport > 0) {
		remainingViewport -= itemSizes[end] || 24;
		end++;
	}

	return { start, end: Math.min(end + buffer, itemSizes.length), offset };
}

// Keyboard navigation utilities
export function handleKeyboardNavigation(
	event: KeyboardEvent,
	selected: [string, string] | null,
	data: any[][],
	shiftPressed: boolean = false
): [string, string] | null {
	if (!selected) return null;

	const current = decodeCell(selected[0]);
	const maxCol = Math.max(0, data[0]?.length - 1 || 0);
	const maxRow = Math.max(0, data.length - 1);

	let newPos = { ...current };

	switch (event.key) {
		case 'ArrowUp':
			newPos.r = Math.max(0, current.r - 1);
			break;
		case 'ArrowDown':
			newPos.r = Math.min(maxRow, current.r + 1);
			break;
		case 'ArrowLeft':
			newPos.c = Math.max(0, current.c - 1);
			break;
		case 'ArrowRight':
			newPos.c = Math.min(maxCol, current.c + 1);
			break;
		case 'Enter':
			newPos.r = Math.min(maxRow, current.r + 1);
			break;
		case 'Tab':
			newPos.c = Math.min(maxCol, current.c + 1);
			if (newPos.c === maxCol && current.c === maxCol) {
				newPos.c = 0;
				newPos.r = Math.min(maxRow, current.r + 1);
			}
			break;
		default:
			return selected;
	}

	const newAddress = encodeCell(newPos);

	if (shiftPressed && selected[1]) {
		return [selected[0], newAddress];
	} else {
		return [newAddress, newAddress];
	}
}

// Data validation and normalization
export function normalizeSpreadsheetData(
	data: any[][],
	minRows: number = 10,
	minCols: number = 10
): any[][] {
	const normalizedData = [...data];

	// Ensure minimum rows
	while (normalizedData.length < minRows) {
		normalizedData.push([]);
	}

	// Ensure minimum columns in each row
	for (let i = 0; i < normalizedData.length; i++) {
		if (!normalizedData[i]) {
			normalizedData[i] = [];
		}

		while (normalizedData[i].length < minCols) {
			normalizedData[i].push('');
		}
	}

	return normalizedData;
}