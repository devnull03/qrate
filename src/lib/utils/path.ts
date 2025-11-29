/**
 * Extract the file name from a full file path
 * Handles both Windows (\) and Unix (/) path separators
 */
export function getFileName(path: string): string {
	const parts = path.split(/[\\/]/);
	return parts[parts.length - 1] || path;
}

/**
 * Extract the file name without extension
 */
export function getFileNameWithoutExtension(path: string): string {
	const fileName = getFileName(path);
	const lastDot = fileName.lastIndexOf(".");
	return lastDot > 0 ? fileName.substring(0, lastDot) : fileName;
}

/**
 * Extract the file extension (including the dot)
 */
export function getFileExtension(path: string): string {
	const fileName = getFileName(path);
	const lastDot = fileName.lastIndexOf(".");
	return lastDot > 0 ? fileName.substring(lastDot) : "";
}

/**
 * Extract the directory path from a full file path
 */
export function getDirectory(path: string): string {
	const lastSeparator = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
	return lastSeparator > 0 ? path.substring(0, lastSeparator) : "";
}
