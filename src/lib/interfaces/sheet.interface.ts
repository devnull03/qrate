export const DELIMITER = ',';

export type Image = [string, FileSystemFileHandle];

// Extended interfaces for spreadsheet functionality
export interface SpreadsheetColumn {
	title?: string;
	width?: string | number;
	readOnly?: boolean;
	type?: 'text' | 'number' | 'boolean' | 'hidden';
	align?: 'left' | 'center' | 'right';
}

export interface SpreadsheetRow {
	height?: string | number;
	data?: any[];
}

export interface SpreadsheetCell {
	value: any;
	style?: string;
	formula?: string;
}

export interface SpreadsheetData {
	data: any[][];
	columns: SpreadsheetColumn[];
	rows: SpreadsheetRow[];
	mergeCells?: Record<string, number[]>;
	style?: Record<string, string>;
	selected?: [string, string];
	currentValue?: string | number | boolean;
}

export interface Sheet {
	fields: Field[];
	rows: Record<string, any>[];
	images: Image[];
}

export interface Field {
	title: string;
	instructions?: string;
}

// AI-generated metadata structure (matches server response)
export interface AIMetadata {
	fileTitle: string;
	title: string;
	field_linked_agent: string;
	field_extent: string;
	field_description: string;
	field_rights: string;
	field_resource_type: string;
	field_language: string;
	field_note: string;
	field_subject: string;
	field_subjects_name: string;
	field_subject_name__organization: string;
	field_geographic_subject: string;
	field_coordinates: string;
}

export interface ImageResponse {
	is_done: boolean;
	metadata: AIMetadata;
	questions: string[];
}

export class Sheet_ implements Sheet {
	fields: Field[]
	rows: Record<string, any>[]
	images: Image[]

	constructor() {
		this.fields = [];
		this.rows = [];
		this.images = [];
	}

	from_new(images: Image[]) {
		this.fields.push({ title: 'file', instructions: 'File names must match exactly with uploaded images.' });
		this.images = images.sort((a, b) => {
			if (a[1].name > b[1].name) return -1;
			if (a[1].name < b[1].name) return 1;
			return 0;
		});

		// Generate rows manually without csv package
		this.rows = images.map((image, index) => ({
			file: image[1].name
		}));

		console.log(this.rows);

		return this;
	}

	// Convert to spreadsheet format
	toSpreadsheetData(): SpreadsheetData {
		const columns: SpreadsheetColumn[] = this.fields.map(field => ({
			title: field.title,
			width: '150px',
			type: 'text'
		}));

		const data: any[][] = this.rows.map(row =>
			this.fields.map(field => row[field.title] || '')
		);

		return {
			data,
			columns,
			rows: data.map(() => ({ height: '24px' })),
			mergeCells: {},
			style: {},
			selected: undefined,
			currentValue: ''
		};
	}

	// Convert from spreadsheet format
	fromSpreadsheetData(spreadsheetData: SpreadsheetData): this {
		this.fields = spreadsheetData.columns.map(col => ({
			title: col.title || '',
			instructions: undefined
		}));

		this.rows = spreadsheetData.data.map(row => {
			const rowObject: Record<string, any> = {};
			row.forEach((value, index) => {
				const fieldTitle = this.fields[index]?.title;
				if (fieldTitle) {
					rowObject[fieldTitle] = value;
				}
			});
			return rowObject;
		});

		return this;
	}

}


