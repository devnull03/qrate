// Re-export everything from utils folder for backward compatibility
export {
	cn,
	type WithoutChild,
	type WithoutChildren,
	type WithoutChildrenOrChild,
	type WithElementRef,
} from "./utils/cn";

export {
	getFileName,
	getFileNameWithoutExtension,
	getFileExtension,
	getDirectory,
} from "./utils/path";

export {
	getAppWindow,
	minimizeWindow,
	toggleMaximizeWindow,
	closeWindow,
	isWindowMaximized,
	isWindowMinimized,
	isWindowVisible,
	setWindowTitle,
} from "./utils/window";
