import { invoke, convertFileSrc } from "@tauri-apps/api/core";

export async function getThumbnailPath(filePath: string): Promise<string> {
	return invoke<string>("get_thumbnail_path", { filePath });
}

export async function getThumbnailUrl(filePath: string): Promise<string> {
	const thumbPath = await getThumbnailPath(filePath);
	return convertFileSrc(thumbPath);
}

export function getAssetUrl(filePath: string): string {
	return convertFileSrc(filePath);
}
