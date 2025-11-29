<script lang="ts">
	import { getCurrentWindow } from "@tauri-apps/api/window";
	import { open, save } from "@tauri-apps/plugin-dialog";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
	import MinusIcon from "@lucide/svelte/icons/minus";
	import SquareIcon from "@lucide/svelte/icons/square";
	import XIcon from "@lucide/svelte/icons/x";

	const appWindow = getCurrentWindow();

	let isProcessing = $state(false);

	// File menu actions
	async function handleNew() {
		if (isProcessing) return;
		try {
			isProcessing = true;
			const selected = await save({
				filters: [{ name: "Qrate Files", extensions: ["qrate"] }],
				defaultPath: "untitled.qrate",
			});
			if (selected) {
				await qrateStore.createFile(selected);
			}
		} catch (err) {
			console.error("Failed to create new file:", err);
		} finally {
			isProcessing = false;
		}
	}

	async function handleOpen() {
		if (isProcessing) return;
		try {
			isProcessing = true;
			const selected = await open({
				multiple: false,
				filters: [{ name: "Qrate Files", extensions: ["qrate"] }],
			});
			if (selected && typeof selected === "string") {
				await qrateStore.openFile(selected);
			}
		} catch (err) {
			console.error("Failed to open file:", err);
		} finally {
			isProcessing = false;
		}
	}

	async function handleSave() {
		// SQLite auto-saves
		console.log("Save requested - changes are auto-saved");
	}

	async function handleSaveAs() {
		// TODO: Implement save as (copy database to new location)
		console.log("Save As requested");
	}

	async function handleImportCsv() {
		if (isProcessing) return;
		try {
			isProcessing = true;
			const csvFile = await open({
				multiple: false,
				filters: [{ name: "CSV Files", extensions: ["csv"] }],
			});
			if (!csvFile || typeof csvFile !== "string") return;

			const qrateFile = await save({
				filters: [{ name: "Qrate Files", extensions: ["qrate"] }],
				defaultPath: csvFile.replace(/\.csv$/i, ".qrate"),
			});
			if (!qrateFile) return;

			await qrateStore.importCsv(qrateFile, csvFile);
		} catch (err) {
			console.error("Failed to import CSV:", err);
		} finally {
			isProcessing = false;
		}
	}

	async function handleClose() {
		try {
			await qrateStore.closeFile();
		} catch (err) {
			console.error("Failed to close file:", err);
		}
	}

	async function handleQuit() {
		await appWindow.close();
	}

	// Edit menu actions
	function handleUndo() {
		document.execCommand("undo");
	}

	function handleRedo() {
		document.execCommand("redo");
	}

	function handleCut() {
		document.execCommand("cut");
	}

	function handleCopy() {
		document.execCommand("copy");
	}

	function handlePaste() {
		document.execCommand("paste");
	}

	function handleSelectAll() {
		document.execCommand("selectAll");
	}

	// Window controls
	function minimize() {
		appWindow.minimize();
	}

	function toggleMaximize() {
		appWindow.toggleMaximize();
	}

	function close() {
		appWindow.close();
	}
</script>

<div class="titlebar" data-tauri-drag-region>
	<div class="titlebar-menu">
		<DropdownMenu.Root>
			<DropdownMenu.Trigger class="menu-trigger"
				>File</DropdownMenu.Trigger
			>
			<DropdownMenu.Content align="start" class="min-w-40">
				<DropdownMenu.Item onclick={handleNew} disabled={isProcessing}>
					New
					<DropdownMenu.Shortcut>Ctrl+N</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Item onclick={handleOpen} disabled={isProcessing}>
					Open
					<DropdownMenu.Shortcut>Ctrl+O</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Separator />
				<DropdownMenu.Item
					onclick={handleSave}
					disabled={!qrateStore.isFileOpen}
				>
					Save
					<DropdownMenu.Shortcut>Ctrl+S</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Item
					onclick={handleSaveAs}
					disabled={!qrateStore.isFileOpen}
				>
					Save As...
					<DropdownMenu.Shortcut>Ctrl+Shift+S</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Separator />
				<DropdownMenu.Item
					onclick={handleImportCsv}
					disabled={isProcessing}
				>
					Import CSV...
				</DropdownMenu.Item>
				<DropdownMenu.Separator />
				<DropdownMenu.Item
					onclick={handleClose}
					disabled={!qrateStore.isFileOpen}
				>
					Close
				</DropdownMenu.Item>
				<DropdownMenu.Item onclick={handleQuit}>
					Quit
					<DropdownMenu.Shortcut>Alt+F4</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>

		<DropdownMenu.Root>
			<DropdownMenu.Trigger class="menu-trigger"
				>Edit</DropdownMenu.Trigger
			>
			<DropdownMenu.Content align="start" class="min-w-40">
				<DropdownMenu.Item onclick={handleUndo}>
					Undo
					<DropdownMenu.Shortcut>Ctrl+Z</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Item onclick={handleRedo}>
					Redo
					<DropdownMenu.Shortcut>Ctrl+Y</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Separator />
				<DropdownMenu.Item onclick={handleCut}>
					Cut
					<DropdownMenu.Shortcut>Ctrl+X</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Item onclick={handleCopy}>
					Copy
					<DropdownMenu.Shortcut>Ctrl+C</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Item onclick={handlePaste}>
					Paste
					<DropdownMenu.Shortcut>Ctrl+V</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Separator />
				<DropdownMenu.Item onclick={handleSelectAll}>
					Select All
					<DropdownMenu.Shortcut>Ctrl+A</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	</div>

	<div class="titlebar-title" data-tauri-drag-region>
		{#if qrateStore.currentFilePath}
			{qrateStore.currentFilePath.split(/[\\/]/).pop()} - qRate
		{:else}
			qRate
		{/if}
	</div>

	<div class="titlebar-controls">
		<button class="control-button" onclick={minimize} title="Minimize">
			<MinusIcon class="size-4" />
		</button>
		<button
			class="control-button"
			onclick={toggleMaximize}
			title="Maximize"
		>
			<SquareIcon class="size-3.5" />
		</button>
		<button
			class="control-button control-close"
			onclick={close}
			title="Close"
		>
			<XIcon class="size-4" />
		</button>
	</div>
</div>

<style>
	.titlebar {
		height: 32px;
		background-color: var(--background);
		user-select: none;
		display: flex;
		align-items: center;
		justify-content: space-between;
		border-bottom: 1px solid var(--border);
		flex-shrink: 0;
	}

	.titlebar-menu {
		display: flex;
		align-items: center;
		height: 100%;
		padding-left: 0.25rem;
	}

	:global(.menu-trigger) {
		height: 100%;
		padding: 0 0.75rem;
		font-size: 0.8125rem;
		background: transparent;
		border: none;
		cursor: pointer;
		display: flex;
		align-items: center;
		color: var(--foreground);
	}

	:global(.menu-trigger:hover) {
		background-color: var(--accent);
	}

	.titlebar-title {
		position: absolute;
		left: 50%;
		transform: translateX(-50%);
		font-size: 0.75rem;
		color: var(--muted-foreground);
		pointer-events: none;
	}

	.titlebar-controls {
		display: flex;
		height: 100%;
	}

	.control-button {
		width: 46px;
		height: 100%;
		display: flex;
		align-items: center;
		justify-content: center;
		background: transparent;
		border: none;
		cursor: pointer;
		color: var(--foreground);
	}

	.control-button:hover {
		background-color: var(--accent);
	}

	.control-close:hover {
		background-color: #e81123;
		color: white;
	}
</style>
