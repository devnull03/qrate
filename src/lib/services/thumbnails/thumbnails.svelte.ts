import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

export interface ThumbnailProgress {
	total: number;
	processed: number;
	complete: boolean;
}

function createThumbnailService() {
	let progress = $state<ThumbnailProgress>({
		total: 0,
		processed: 0,
		complete: true,
	});
	let isProcessing = $state(false);

	let progressUnlisten: UnlistenFn | null = null;
	let completeUnlisten: UnlistenFn | null = null;

	async function initListeners() {
		await cleanupListeners();

		progressUnlisten = await listen<ThumbnailProgress>(
			"thumbnail-progress",
			(event) => {
				progress = event.payload;
				isProcessing = !event.payload.complete;
			},
		);

		completeUnlisten = await listen<ThumbnailProgress>(
			"thumbnail-complete",
			(event) => {
				progress = event.payload;
				isProcessing = false;
			},
		);
	}

	async function cleanupListeners() {
		progressUnlisten?.();
		completeUnlisten?.();
		progressUnlisten = null;
		completeUnlisten = null;
	}

	async function startProcessing(filesFolder: string) {
		isProcessing = true;
		await initListeners();
		progress = await invoke<ThumbnailProgress>(
			"start_thumbnail_processing",
			{ filesFolder },
		);
	}

	async function cancel() {
		await invoke("cancel_thumbnail_processing");
	}

	async function dispose() {
		await cleanupListeners();
	}

	return {
		get progress() {
			return progress;
		},
		get isProcessing() {
			return isProcessing;
		},
		startProcessing,
		cancel,
		dispose,
	};
}

export const thumbnailService = createThumbnailService();
