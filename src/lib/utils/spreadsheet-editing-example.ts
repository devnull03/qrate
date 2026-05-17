/**
 * Example usage of the enhanced spreadsheet editing functions
 * 
 * This file demonstrates how to use the new editing functions in your components
 */

import { createSpreadsheetEditingAPI, editCell, editRow, addRow, deleteRow } from './files';
import type { Sheet } from '$lib/interfaces/sheet.interface';

// Example 1: Basic cell editing with validation
export async function exampleCellEdit(sheet: Sheet) {
	// Edit a single cell with validation
	const result = await editCell(sheet, 0, 'title', 'Updated Title');

	if (result.success) {
		console.log('Cell updated successfully');
		// Use the updated sheet
		const updatedSheet = result.sheet!;
	} else {
		console.error('Validation failed:', result.error);
	}
}

// Example 2: Row editing with multiple field validation
export async function exampleRowEdit(sheet: Sheet) {
	const rowData = {
		title: 'New Title',
		description: 'A description of this item',
		file: 'image.jpg'
	};

	const result = await editRow(sheet, 0, rowData);

	if (result.success) {
		console.log('Row updated successfully');
		const updatedSheet = result.sheet!;
	} else {
		console.error('Validation errors:', result.errors);
	}
}

// Example 3: Using the convenient API wrapper
export function exampleUsingAPI(sheet: Sheet) {
	const api = createSpreadsheetEditingAPI(sheet);

	// All operations automatically update the original sheet and autosave
	return {
		// Edit a cell
		editCell: async (row: number, field: string, value: any) => {
			return await api.editCell(row, field, value);
		},

		// Edit multiple cells at once
		editMultipleCells: async () => {
			return await api.batchEdit([
				{ rowIndex: 0, fieldName: 'title', value: 'Title 1' },
				{ rowIndex: 1, fieldName: 'title', value: 'Title 2' },
				{ rowIndex: 0, fieldName: 'description', value: 'Description 1' }
			]);
		},

		// Add a new row
		addNewRow: async () => {
			return await api.addRow({
				title: 'New Item',
				description: 'New description',
				file: 'new-image.jpg'
			});
		},

		// Delete a row
		removeRow: async (rowIndex: number) => {
			return await api.deleteRow(rowIndex);
		},

		// Validate before editing
		checkValue: (field: string, value: any) => {
			return api.validateValue(field, value);
		},

		// Force immediate save (bypass debouncing)
		saveNow: () => api.forceSave()
	};
}

// Example 4: Integration with Svelte component
export function createSpreadsheetStore(initialSheet: Sheet) {
	let sheet = $state(initialSheet);
	const api = createSpreadsheetEditingAPI(sheet);

	return {
		get sheet() { return sheet; },

		// Reactive editing functions that update the store
		editCell: async (rowIndex: number, fieldName: string, value: any) => {
			const result = await api.editCell(rowIndex, fieldName, value);
			if (result.success) {
				// Trigger Svelte reactivity
				sheet = { ...sheet };
			}
			return result;
		},

		editRow: async (rowIndex: number, rowData: Record<string, any>) => {
			const result = await api.editRow(rowIndex, rowData);
			if (result.success) {
				sheet = { ...sheet };
			}
			return result;
		},

		addRow: async (rowData: Record<string, any> = {}) => {
			await api.addRow(rowData);
			sheet = { ...sheet };
			return { success: true };
		},

		deleteRow: async (rowIndex: number) => {
			const result = await api.deleteRow(rowIndex);
			if (result.success) {
				sheet = { ...sheet };
			}
			return result;
		}
	};
}

// Example 5: Validation showcase
export function showcaseValidation(sheet: Sheet) {
	const api = createSpreadsheetEditingAPI(sheet);

	// Test different types of validation
	const tests = [
		{ field: 'email', value: 'test@example.com', shouldPass: true },
		{ field: 'email', value: 'invalid-email', shouldPass: false },
		{ field: 'field_coordinates', value: '45.123', shouldPass: true },
		{ field: 'field_coordinates', value: 'not-a-number', shouldPass: false },
		{ field: 'file', value: 'valid-filename.jpg', shouldPass: true },
		{ field: 'file', value: 'invalid<file>name.jpg', shouldPass: false }
	];

	console.log('Validation Test Results:');
	tests.forEach(test => {
		const result = api.validateValue(test.field, test.value);
		const passed = result.isValid === test.shouldPass;
		console.log(`${test.field}: "${test.value}" - ${passed ? '✓' : '✗'} ${result.error || ''}`);
	});
}