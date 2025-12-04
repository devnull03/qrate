import { trace, debug, info, warn, error } from "@tauri-apps/plugin-log";

export function forwardConsole() {
	const original = {
		log: console.log,
		debug: console.debug,
		info: console.info,
		warn: console.warn,
		error: console.error,
	};

	console.log = (...args) => {
		original.log(...args);
		trace(args.map(String).join(" "));
	};

	console.debug = (...args) => {
		original.debug(...args);
		debug(args.map(String).join(" "));
	};

	console.info = (...args) => {
		original.info(...args);
		info(args.map(String).join(" "));
	};

	console.warn = (...args) => {
		original.warn(...args);
		warn(args.map(String).join(" "));
	};

	console.error = (...args) => {
		original.error(...args);
		error(args.map(String).join(" "));
	};
}

export { trace, debug, info, warn, error };
