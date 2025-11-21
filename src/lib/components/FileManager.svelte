<script lang="ts">
	import { open, save } from '@tauri-apps/plugin-dialog';
	import { qrateStore } from '$lib/stores/qrateStore.svelte';
	import { Button } from '$lib/components/ui/button';

	let isProcessing = $state(false);

	/**
	 * Open an existing .qrate file
	 */
	async function handleOpenQrate() {
		try {
			isProcessing = true;

			const selected = await open({
				multiple: false,
				filters: [
					{
						name: 'Qrate Files',
						extensions: ['qrate'],
					},
				],
			});

			if (selected && typeof selected === 'string') {
				await qrateStore.openFile(selected);
			}
		} catch (err) {
			console.error('Failed to open .qrate file:', err);
		} finally {
			isProcessing = false;
		}
	}

	/**
	 * Create a new .qrate file
	 */
	async function handleCreateQrate() {
		try {
			isProcessing = true;

			const selected = await save({
				filters: [
					{
						name: 'Qrate Files',
						extensions: ['qrate'],
					},
				],
				defaultPath: 'untitled.qrate',
			});

			if (selected) {
				await qrateStore.createFile(selected);
			}
		} catch (err) {
			console.error('Failed to create .qrate file:', err);
		} finally {
			isProcessing = false;
		}
	}

	/**
	 * Import a CSV file into a new or existing .qrate file
	 */
	async function handleImportCsv() {
		try {
			isProcessing = true;

			// First, select the CSV file to import
			const csvFile = await open({
				multiple: false,
				filters: [
					{
						name: 'CSV Files',
						extensions: ['csv'],
					},
				],
			});

			if (!csvFile || typeof csvFile !== 'string') {
				isProcessing = false;
				return;
			}

			// Then, select where to save the .qrate file
			const qrateFile = await save({
				filters: [
					{
						name: 'Qrate Files',
						extensions: ['qrate'],
					},
				],
				defaultPath: csvFile.replace(/\.csv$/i, '.qrate'),
			});

			if (!qrateFile) {
				isProcessing = false;
				return;
			}

			// Import the CSV data
			await qrateStore.importCsv(qrateFile, csvFile);
		} catch (err) {
			console.error('Failed to import CSV:', err);
		} finally {
			isProcessing = false;
		}
	}

	/**
	 * Close the current file
	 */
	async function handleCloseFile() {
		try {
			await qrateStore.closeFile();
		} catch (err) {
			console.error('Failed to close file:', err);
		}
	}

	// Current file info
	let currentFileName = $derived.by(() => {
		if (!qrateStore.currentFilePath) return null;
		const parts = qrateStore.currentFilePath.split(/[\\/]/);
		return parts[parts.length - 1];
	});

	let fileStats = $derived.by(() => {
		if (!qrateStore.isFileOpen) return null;
		return {
			rows: qrateStore.totalRows,
			columns: qrateStore.columns.length,
		};
	});
</script>

<div class="flex flex-col h-full border-r border-border bg-background p-4 space-y-4 min-w-[250px]">
	<div class="space-y-2">
		<h2 class="text-lg font-semibold">File</h2>

		{#if qrateStore.isFileOpen && currentFileName}
			<div class="p-3 bg-muted rounded-md space-y-1">
				<p class="text-sm font-medium truncate" title={qrateStore.currentFilePath}>
					{currentFileName}
				</p>
				{#if fileStats}
					<p class="text-xs text-muted-foreground">
						{fileStats.rows.toLocaleString()} rows × {fileStats.columns} columns
					</p>
				{/if}
			</div>
		{/if}
	</div>

	<div class="space-y-2">
		{#if !qrateStore.isFileOpen}
			<Button
				onclick={handleOpenQrate}
				disabled={isProcessing}
				class="w-full"
				variant="outline"
			>
				Open .qrate File
			</Button>

			<Button
				onclick={handleCreateQrate}
				disabled={isProcessing}
				class="w-full"
				variant="outline"
			>
				New .qrate File
			</Button>

			<div class="relative">
				<div class="absolute inset-0 flex items-center">
					<span class="w-full border-t border-border"></span>
				</div>
				<div class="relative flex justify-center text-xs uppercase">
					<span class="bg-background px-2 text-muted-foreground">Or</span>
				</div>
			</div>

			<Button
				onclick={handleImportCsv}
				disabled={isProcessing}
				class="w-full"
			>
				Import CSV
			</Button>
		{:else}
			<Button
				onclick={handleCloseFile}
				disabled={isProcessing}
				class="w-full"
				variant="outline"
			>
				Close File
			</Button>
		{/if}
	</div>

	{#if isProcessing}
		<div class="text-sm text-muted-foreground text-center">
			Processing...
		</div>
	{/if}

	<div class="flex-1"></div>

	<div class="space-y-2 text-xs text-muted-foreground pt-4 border-t border-border">
		<h3 class="font-semibold text-foreground">About .qrate Format</h3>
		<p>
			.qrate files are SQLite databases that provide:
		</p>
		<ul class="list-disc list-inside space-y-1 ml-2">
			<li>Instant loading of large datasets</li>
			<li>ACID transaction safety</li>
			<li>Column metadata persistence</li>
			<li>Virtual scrolling support</li>
		</ul>
	</div>
</div>

<style>
	/* Additional styling can go here if needed */
</style>
