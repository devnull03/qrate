import { invoke } from '@tauri-apps/api/core';

type CsvData = Record<string, string>[];

class CsvStore {
	data = $state<CsvData>([]);
	isLoading = $state(false);
	error = $state<string | null>(null);

	async loadCsv(path: string) {
		this.isLoading = true;
		this.error = null;

		try {
			const result = await invoke<CsvData>('load_csv', { path });
			this.data = result;
		} catch (err) {
			this.error = err as string;
			console.error('Failed to load CSV:', err);
		} finally {
			this.isLoading = false;
		}
	}

	clear() {
		this.data = [];
		this.error = null;
	}
}

export const csvStore = new CsvStore();
