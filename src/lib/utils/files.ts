/**
 * Spreadsheet editing utilities with validation and autosave
 * 
 * This file provides comprehensive functions for editing spreadsheet data:
 * - Cell-level editing with validation and type coercion
 * - Row-level editing with batch validation  
 * - Row operations (add, delete, move)
 * - Debounced autosave to IndexedDB
 * - Data validation helpers
 * - Batch editing for performance
 * 
 * All editing functions include automatic validation and optional autosave.
 * Use createSpreadsheetEditingAPI() for a convenient wrapper around all functions.
 */

import type { Image, Sheet, Field } from "$lib/interfaces/sheet.interface";
import * as kv from "idb-keyval";

// File operations
export const getUploadedFiles = async () => {
    const dir = (await kv.get("images")) as FileSystemDirectoryHandle;
    if (!dir) return null;
    await dir.requestPermission();
    const entries = [];
    for await (const entry of dir.entries()) {
        if (entry[1].kind !== "file") continue;
        entries.push(entry as Image);
    }
    return entries;
};

// Storage operations for spreadsheet data
export const getStoredSpreadsheet = async (): Promise<Sheet | null> => {
    const sheet = await kv.get("sheet");
    if (!sheet) return null;
    return sheet as Sheet;
};

export const setStoredSpreadsheet = async (data: Sheet) => {
    await kv.set("sheet", data);
};

// Debounced autosave utility
let saveTimeout: ReturnType<typeof setTimeout> | null = null;

const debouncedSave = (data: Sheet, delay: number = 500) => {
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(async () => {
        await setStoredSpreadsheet(data);
        console.log('Spreadsheet autosaved');
    }, delay);
};

// Cell editing function with validation
export const editCell = async (
    sheet: Sheet,
    rowIndex: number,
    fieldName: string,
    value: any,
    autoSave: boolean = true
): Promise<{ success: boolean; error?: string; sheet?: Sheet }> => {
    // Validate row index
    if (rowIndex < 0 || rowIndex >= sheet.rows.length) {
        return { success: false, error: `Invalid row index: ${rowIndex}` };
    }

    // Validate field exists
    const field = sheet.fields.find(f => f.title === fieldName);
    if (!field) {
        return { success: false, error: `Field '${fieldName}' not found` };
    }

    // Validate the value
    const validation = validateCellValue(value, field);
    if (!validation.isValid) {
        return { success: false, error: validation.error };
    }

    // Create a deep copy of the sheet
    const updatedSheet = {
        ...sheet,
        rows: [...sheet.rows]
    };

    // Update the specific cell with validated/coerced value
    updatedSheet.rows[rowIndex] = {
        ...updatedSheet.rows[rowIndex],
        [fieldName]: validation.coercedValue !== undefined ? validation.coercedValue : value
    };

    // Auto-save if requested
    if (autoSave) {
        debouncedSave(updatedSheet);
    }

    return { success: true, sheet: updatedSheet };
};

// Row editing function with validation
export const editRow = async (
    sheet: Sheet,
    rowIndex: number,
    rowData: Record<string, any>,
    autoSave: boolean = true
): Promise<{ success: boolean; errors?: Record<string, string>; sheet?: Sheet }> => {
    // Validate row index
    if (rowIndex < 0 || rowIndex >= sheet.rows.length) {
        return { success: false, errors: { general: `Invalid row index: ${rowIndex}` } };
    }

    // Validate all row data
    const validation = validateRowData(rowData, sheet.fields);
    if (!validation.isValid) {
        return { success: false, errors: validation.errors };
    }

    // Create a deep copy of the sheet
    const updatedSheet = {
        ...sheet,
        rows: [...sheet.rows]
    };

    // Update the entire row with validated/coerced data
    updatedSheet.rows[rowIndex] = {
        ...updatedSheet.rows[rowIndex],
        ...validation.coercedData
    };

    // Auto-save if requested
    if (autoSave) {
        debouncedSave(updatedSheet);
    }

    return { success: true, sheet: updatedSheet };
};

// Add a new row
export const addRow = async (
    sheet: Sheet,
    rowData: Record<string, any> = {},
    insertIndex?: number,
    autoSave: boolean = true
): Promise<Sheet> => {
    // Create a deep copy of the sheet
    const updatedSheet = {
        ...sheet,
        rows: [...sheet.rows]
    };

    // Create new row with default values
    const newRow: Record<string, any> = {};
    sheet.fields.forEach(field => {
        newRow[field.title] = rowData[field.title] || '';
    });

    // Insert at specified index or append to end
    if (insertIndex !== undefined && insertIndex >= 0 && insertIndex <= sheet.rows.length) {
        updatedSheet.rows.splice(insertIndex, 0, newRow);
    } else {
        updatedSheet.rows.push(newRow);
    }

    // Auto-save if requested
    if (autoSave) {
        debouncedSave(updatedSheet);
    }

    return updatedSheet;
};

// Delete a row
export const deleteRow = async (
    sheet: Sheet,
    rowIndex: number,
    autoSave: boolean = true
): Promise<Sheet> => {
    // Validate row index
    if (rowIndex < 0 || rowIndex >= sheet.rows.length) {
        throw new Error(`Invalid row index: ${rowIndex}`);
    }

    // Create a deep copy of the sheet
    const updatedSheet = {
        ...sheet,
        rows: [...sheet.rows]
    };

    // Remove the row
    updatedSheet.rows.splice(rowIndex, 1);

    // Auto-save if requested
    if (autoSave) {
        debouncedSave(updatedSheet);
    }

    return updatedSheet;
};

// Move a row to a new position
export const moveRow = async (
    sheet: Sheet,
    fromIndex: number,
    toIndex: number,
    autoSave: boolean = true
): Promise<Sheet> => {
    // Validate indices
    if (fromIndex < 0 || fromIndex >= sheet.rows.length ||
        toIndex < 0 || toIndex >= sheet.rows.length) {
        throw new Error(`Invalid row indices: from=${fromIndex}, to=${toIndex}`);
    }

    if (fromIndex === toIndex) {
        return sheet; // No change needed
    }

    // Create a deep copy of the sheet
    const updatedSheet = {
        ...sheet,
        rows: [...sheet.rows]
    };

    // Move the row
    const [movedRow] = updatedSheet.rows.splice(fromIndex, 1);
    updatedSheet.rows.splice(toIndex, 0, movedRow);

    // Auto-save if requested
    if (autoSave) {
        debouncedSave(updatedSheet);
    }

    return updatedSheet;
};

// Data validation helpers
export const validateCellValue = (value: any, field: Field): { isValid: boolean; error?: string; coercedValue?: any } => {
    // Basic validation - can be extended based on field instructions
    if (value === null || value === undefined) {
        return { isValid: true, coercedValue: '' };
    }

    // Convert to string for basic validation
    const stringValue = String(value).trim();

    // Check field-specific validation based on field name patterns
    const fieldName = field.title.toLowerCase();

    // Email validation
    if (fieldName.includes('email')) {
        const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
        if (!emailRegex.test(stringValue)) {
            return { isValid: false, error: 'Invalid email format' };
        }
    }

    // URL validation
    if (fieldName.includes('url') || fieldName.includes('link')) {
        try {
            new URL(stringValue);
        } catch {
            return { isValid: false, error: 'Invalid URL format' };
        }
    }

    // Coordinate validation
    if (fieldName.includes('coordinate') || fieldName.includes('lat') || fieldName.includes('lng')) {
        const num = parseFloat(stringValue);
        if (isNaN(num)) {
            return { isValid: false, error: 'Coordinates must be numeric' };
        }
        return { isValid: true, coercedValue: num };
    }

    // File name validation
    if (fieldName === 'file' || fieldName.includes('filename')) {
        if (stringValue.length === 0) {
            return { isValid: false, error: 'File name cannot be empty' };
        }
        // Check for invalid characters
        const invalidChars = /[<>:"/\\|?*]/;
        if (invalidChars.test(stringValue)) {
            return { isValid: false, error: 'File name contains invalid characters' };
        }
    }

    return { isValid: true, coercedValue: stringValue };
};

export const validateRowData = (rowData: Record<string, any>, fields: Field[]): { isValid: boolean; errors: Record<string, string>; coercedData: Record<string, any> } => {
    const errors: Record<string, string> = {};
    const coercedData: Record<string, any> = {};

    for (const field of fields) {
        const value = rowData[field.title];
        const validation = validateCellValue(value, field);

        if (!validation.isValid) {
            errors[field.title] = validation.error || 'Invalid value';
        } else {
            coercedData[field.title] = validation.coercedValue !== undefined ? validation.coercedValue : value;
        }
    }

    return {
        isValid: Object.keys(errors).length === 0,
        errors,
        coercedData
    };
};

// Batch operations for better performance
export const batchEditCells = async (
    sheet: Sheet,
    edits: Array<{ rowIndex: number; fieldName: string; value: any }>,
    autoSave: boolean = true
): Promise<Sheet> => {
    // Create a deep copy of the sheet
    const updatedSheet = {
        ...sheet,
        rows: [...sheet.rows.map(row => ({ ...row }))]
    };

    // Apply all edits
    for (const edit of edits) {
        const { rowIndex, fieldName, value } = edit;

        // Validate indices
        if (rowIndex < 0 || rowIndex >= updatedSheet.rows.length) {
            console.warn(`Invalid row index: ${rowIndex}, skipping edit`);
            continue;
        }

        // Validate field exists
        const field = updatedSheet.fields.find(f => f.title === fieldName);
        if (!field) {
            console.warn(`Field '${fieldName}' not found, skipping edit`);
            continue;
        }

        // Apply edit
        updatedSheet.rows[rowIndex][fieldName] = value;
    }

    // Auto-save if requested
    if (autoSave) {
        debouncedSave(updatedSheet);
    }

    return updatedSheet;
};

// Force immediate save (bypass debouncing)
export const forceSave = async (sheet: Sheet): Promise<void> => {
    if (saveTimeout) {
        clearTimeout(saveTimeout);
        saveTimeout = null;
    }
    await setStoredSpreadsheet(sheet);
    console.log('Spreadsheet force saved');
};

// Example usage functions for easy integration
export const createSpreadsheetEditingAPI = (sheet: Sheet) => {
    return {
        // Edit a single cell
        editCell: async (rowIndex: number, fieldName: string, value: any) => {
            const result = await editCell(sheet, rowIndex, fieldName, value);
            if (result.success && result.sheet) {
                // Update the original sheet reference
                Object.assign(sheet, result.sheet);
            }
            return result;
        },

        // Edit an entire row
        editRow: async (rowIndex: number, rowData: Record<string, any>) => {
            const result = await editRow(sheet, rowIndex, rowData);
            if (result.success && result.sheet) {
                Object.assign(sheet, result.sheet);
            }
            return result;
        },

        // Add a new row
        addRow: async (rowData: Record<string, any> = {}, insertIndex?: number) => {
            const updatedSheet = await addRow(sheet, rowData, insertIndex);
            Object.assign(sheet, updatedSheet);
            return { success: true };
        },

        // Delete a row
        deleteRow: async (rowIndex: number) => {
            try {
                const updatedSheet = await deleteRow(sheet, rowIndex);
                Object.assign(sheet, updatedSheet);
                return { success: true };
            } catch (error) {
                return { success: false, error: (error as Error).message };
            }
        },

        // Move a row
        moveRow: async (fromIndex: number, toIndex: number) => {
            try {
                const updatedSheet = await moveRow(sheet, fromIndex, toIndex);
                Object.assign(sheet, updatedSheet);
                return { success: true };
            } catch (error) {
                return { success: false, error: (error as Error).message };
            }
        },

        // Batch edit multiple cells
        batchEdit: async (edits: Array<{ rowIndex: number; fieldName: string; value: any }>) => {
            const updatedSheet = await batchEditCells(sheet, edits);
            Object.assign(sheet, updatedSheet);
            return { success: true };
        },

        // Force save immediately
        forceSave: () => forceSave(sheet),

        // Get validation errors for a value before editing
        validateValue: (fieldName: string, value: any) => {
            const field = sheet.fields.find(f => f.title === fieldName);
            if (!field) {
                return { isValid: false, error: 'Field not found' };
            }
            return validateCellValue(value, field);
        }
    };
};

// USELESS
export interface ImageResponse {
    is_done: boolean;
    metadata: {
        fileTitle: "";
        title: "[Photograph of a red apple]";
        field_linked_agent: "";
        field_extent: "";
        field_description: "The image depicts a single, whole red apple with a glossy surface and a visible stem. The apple appears fresh and is centered against a plain white background. There are no other objects or elements present in the image.";
        field_rights: "";
        field_resource_type: "still image";
        field_language: "";
        field_note: "";
        field_subject: "Apples; Fruits";
        field_subjects_name: "";
        field_subject_name__organization: "";
        field_geographic_subject: "";
        field_coordinates: "";
    };
    questions: [
        "What is the intended use or context of this image?",
        "Who is the creator or photographer of this image?",
        "Are there any specific notes or additional information about this image that should be included?",
        "What is the language of the description or any associated text?",
        "Is there a specific geographic location or context related to this image?",
        "Are there any rights or licensing information associated with this image?"
    ];
}

// USELESS
export const getImageRes = async (filename: string, base64: string, qna?: [string, string][]) => {
    console.log(base64)
    const data = (JSON.parse(localStorage.imageRes)[filename] ?? null) as null | ImageResponse;
    const dat =
        !data || (!data.is_done && qna)
            ? await (await fetch("/api/image", { method: "post", body: JSON.stringify({ image: base64, qna }) })).json()
            : data;
    const temp = JSON.parse(localStorage.imageRes);
    localStorage.imageRes = JSON.stringify({ ...temp, [filename]: dat });
    return dat;
};
