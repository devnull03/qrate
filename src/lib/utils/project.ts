import type {
	Image,
	Field,
	Sheet,
	AIMetadata,
	ImageResponse
} from "$lib/interfaces/sheet.interface";
import { getUploadedFiles, setStoredSpreadsheet } from "./files";
import * as kv from "idb-keyval";

/**
 * Default fields configuration for new projects
 */
export interface DefaultFieldConfig {
	field: Field;
	generator?: (image: Image, index: number, aiData?: AIMetadata) => any;
}

/**
 * Default fields that are created for new projects (including AI metadata fields)
 */
export const DEFAULT_FIELDS: DefaultFieldConfig[] = [
	{
		field: {
			title: "file",
			instructions: "File names must match exactly with uploaded images."
		},
		generator: (image: Image) => image[1].name
	},
	{
		field: {
			title: "file_extension",
			instructions: "File extension extracted from the filename."
		},
		generator: (image: Image) => {
			const name = image[1].name;
			const lastDot = name.lastIndexOf(".");
			return lastDot !== -1 ? name.substring(lastDot + 1) : "";
		}
	},
	{
		field: {
			title: "accessIdentifier",
			instructions: "Unique identifier for accessing this item."
		},
		generator: () => "" // Empty for now, can be customized later
	},
	// AI-generated metadata fields
	{
		field: {
			title: "fileTitle",
			instructions: "Title of the file as determined by AI analysis."
		},
		generator: (image: Image, index: number, aiData?: AIMetadata) => aiData?.fileTitle || ""
	},
	{
		field: {
			title: "title",
			instructions: "Descriptive title of the image content."
		},
		generator: (image: Image, index: number, aiData?: AIMetadata) => aiData?.title || ""
	},
	{
		field: {
			title: "field_description",
			instructions: "Detailed description of the image content."
		},
		generator: (image: Image, index: number, aiData?: AIMetadata) => aiData?.field_description || ""
	},
	{
		field: {
			title: "field_subject",
			instructions: "Subject or topic of the image."
		},
		generator: (image: Image, index: number, aiData?: AIMetadata) => aiData?.field_subject || ""
	},
	{
		field: {
			title: "field_linked_agent",
			instructions: "Person or organization linked to this resource."
		},
		generator: (image: Image, index: number, aiData?: AIMetadata) =>
			aiData?.field_linked_agent || ""
	},
	{
		field: {
			title: "field_resource_type",
			instructions: "Type of the resource (e.g., still image)."
		},
		generator: (image: Image, index: number, aiData?: AIMetadata) =>
			aiData?.field_resource_type || "still image"
	},
	{
		field: {
			title: "field_rights",
			instructions: "Rights or usage restrictions for the resource."
		},
		generator: (image: Image, index: number, aiData?: AIMetadata) => aiData?.field_rights || ""
	},
	{
		field: {
			title: "field_geographic_subject",
			instructions: "Geographic location associated with the resource."
		},
		generator: (image: Image, index: number, aiData?: AIMetadata) =>
			aiData?.field_geographic_subject || ""
	},
	{
		field: {
			title: "field_coordinates",
			instructions: "Geographic coordinates related to the resource."
		},
		generator: (image: Image, index: number, aiData?: AIMetadata) => aiData?.field_coordinates || ""
	}
];

/**
 * Project creation options
 */
export interface ProjectCreationOptions {
	name?: string;
	defaultFields?: DefaultFieldConfig[];
	sortOrder?: "asc" | "desc";
}

/**
 * Fast check if a project exists in storage
 * @returns Promise<boolean> - true if project exists, false otherwise
 */
export async function projectExists(): Promise<boolean> {
	// Check if we're in a browser environment
	if (typeof window === "undefined") {
		return true;
	}

	try {
		const sheet = await kv.get("sheet");
		return sheet !== undefined && sheet !== null;
	} catch (error) {
		console.error("Failed to check if project exists:", error);
		return false;
	}
}

/**
 * Sort images in the specified order
 */
function sortImages(images: Image[], order: "asc" | "desc" = "desc"): Image[] {
	return images.sort((a, b) => {
		const nameA = a[1].name;
		const nameB = b[1].name;

		if (order === "desc") {
			return nameA > nameB ? -1 : nameA < nameB ? 1 : 0;
		} else {
			return nameA < nameB ? -1 : nameA > nameB ? 1 : 0;
		}
	});
}

/**
 * Generate template spreadsheet data from images and field configurations
 */
function generateTemplateSpreadsheet(
	images: Image[],
	fieldConfigs: DefaultFieldConfig[],
	aiResults?: Map<string, ImageResponse>
): Sheet {
	// Extract fields from configurations
	const fields = fieldConfigs.map((config) => config.field);

	// Generate rows using the generator functions
	const rows = images.map((image, index) => {
		const row: Record<string, any> = {};
		const filename = image[1].name;
		const aiData = aiResults?.get(filename)?.metadata;

		fieldConfigs.forEach((config) => {
			const fieldTitle = config.field.title;
			if (config.generator) {
				row[fieldTitle] = config.generator(image, index, aiData);
			} else {
				row[fieldTitle] = "";
			}
		});

		return row;
	});

	return {
		fields,
		rows,
		images
	};
}

/**
 * Create a new project from scratch using uploaded images
 */
export async function createFromScratch(
	options: ProjectCreationOptions = {},
	aiResults?: Map<string, ImageResponse>
): Promise<Sheet | null> {
	try {
		// Get uploaded files
		const images = await getUploadedFiles();
		if (!images || images.length === 0) {
			console.warn("No uploaded images found");
			return null;
		}

		// Use provided fields or default ones
		const fieldConfigs = options.defaultFields || DEFAULT_FIELDS;

		// Sort images (default: descending)
		const sortOrder = options.sortOrder || "desc";
		const sortedImages = sortImages(images, sortOrder);

		// Generate template spreadsheet with AI data
		const sheet = generateTemplateSpreadsheet(sortedImages, fieldConfigs, aiResults);

		// Store the project
		await setStoredSpreadsheet(sheet);

		// Store AI results for future reference
		if (aiResults) {
			await kv.set("ai-results", Object.fromEntries(aiResults));
		}

		// Optionally store project metadata
		if (options.name) {
			await kv.set("project-name", options.name);
		}

		console.log(`Created project with ${sheet.rows.length} items and AI metadata`);
		return sheet;
	} catch (error) {
		console.error("Failed to create project from scratch:", error);
		return null;
	}
}

/**
 * Create custom field configuration
 */
export function createFieldConfig(
	title: string,
	instructions?: string,
	generator?: (image: Image, index: number) => any
): DefaultFieldConfig {
	return {
		field: { title, instructions },
		generator
	};
}

/**
 * Get default field configurations (useful for extending)
 */
export function getDefaultFields(): DefaultFieldConfig[] {
	return [...DEFAULT_FIELDS];
}

/**
 * Create project with custom fields
 */
export async function createWithCustomFields(
	customFields: DefaultFieldConfig[],
	options: Omit<ProjectCreationOptions, "defaultFields"> = {}
): Promise<Sheet | null> {
	return createFromScratch({
		...options,
		defaultFields: customFields
	});
}

/**
 * Complete project creation workflow with AI metadata generation
 */
export async function createProjectWithAI(
	images: Image[],
	directoryHandle: FileSystemDirectoryHandle,
	options: ProjectCreationOptions = {},
	onProgress?: (current: number, total: number, filename: string) => void
): Promise<Sheet | null> {
	try {
		// Store directory handle first
		await kv.set("images", directoryHandle);

		// Process images with AI
		const aiResults = new Map<string, ImageResponse>();
let i=0;
		await Promise.all(
			images.map(async ([filename, handle], index) => {
				// Notify progress (note: will not be strictly sequential)

				try {
					const file = await handle.getFile();
					const base64 = await blobToBase64(file);

					// Call AI service
					const response = await fetch("/api/image", {
						method: "POST",
						headers: { "Content-Type": "application/json" },
						body: JSON.stringify({ image: base64 })
					});

					onProgress?.(++i, images.length, filename);
					if (response.ok) {
						const data = await response.json();
						aiResults.set(filename, data.response);
					} else {
						console.warn(`Failed to process ${filename} with AI`);
					}
				} catch (error) {
					console.error(`Error processing ${filename}:`, error);
				}
			})
		);

		// Create project with AI results
		return await createFromScratch(options, aiResults);
	} catch (error) {
		console.error("Failed to create project with AI:", error);
		return null;
	}
}

/**
 * Helper function to convert blob to base64
 */
async function blobToBase64(blob: Blob): Promise<string> {
	return new Promise<string>((resolve, reject) => {
		const reader = new FileReader();
		reader.onload = () => {
			const result = reader.result as string;
			resolve(result.replace(/^data:image\/[a-z]+;base64,/, ""));
		};
		reader.onerror = (err) => reject(err);
		reader.readAsDataURL(blob);
	});
}

/**
 * Load existing project from storage
 */
export async function loadProject(): Promise<Sheet | null> {
	try {
		const sheet = await kv.get("sheet");
		return (sheet as Sheet) || null;
	} catch (error) {
		console.error("Failed to load project:", error);
		return null;
	}
}

/**
 * Save project to storage
 */
export async function saveProject(sheet: Sheet): Promise<boolean> {
	try {
		await setStoredSpreadsheet(sheet);
		return true;
	} catch (error) {
		console.error("Failed to save project:", error);
		return false;
	}
}

/**
 * Clear current project
 */
export async function clearProject(): Promise<boolean> {
	try {
		await kv.del("sheet");
		await kv.del("project-name");
		return true;
	} catch (error) {
		console.error("Failed to clear project:", error);
		return false;
	}
}
