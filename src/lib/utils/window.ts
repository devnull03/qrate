import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Get the current Tauri window instance
 */
export function getAppWindow() {
	return getCurrentWindow();
}

/**
 * Minimize the current window
 */
export async function minimizeWindow(): Promise<void> {
	const appWindow = getCurrentWindow();
	await appWindow.minimize();
}

/**
 * Toggle maximize/restore for the current window
 */
export async function toggleMaximizeWindow(): Promise<void> {
	const appWindow = getCurrentWindow();
	await appWindow.toggleMaximize();
}

/**
 * Close the current window
 */
export async function closeWindow(): Promise<void> {
	const appWindow = getCurrentWindow();
	await appWindow.close();
}
/**
 * Hide the current window
 */
export function hideWindow() {
  const appWindow = getCurrentWindow();
  appWindow.hide();
}



/**
 * Check if the current window is maximized
 */
export async function isWindowMaximized(): Promise<boolean> {
	const appWindow = getCurrentWindow();
	return await appWindow.isMaximized();
}

/**
 * Check if the current window is minimized
 */
export async function isWindowMinimized(): Promise<boolean> {
	const appWindow = getCurrentWindow();
	return await appWindow.isMinimized();
}

/**
 * Check if the current window is visible
 */
export async function isWindowVisible(): Promise<boolean> {
	const appWindow = getCurrentWindow();
	return await appWindow.isVisible();
}

/**
 * Set the window title
 */
export async function setWindowTitle(title: string): Promise<void> {
	const appWindow = getCurrentWindow();
	await appWindow.setTitle(title);
}
