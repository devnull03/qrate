<script lang="ts">
	import { invoke } from "@tauri-apps/api/core";
	import { open, save } from "@tauri-apps/plugin-dialog";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import MinusIcon from "@lucide/svelte/icons/minus";
	import SquareIcon from "@lucide/svelte/icons/square";
	import XIcon from "@lucide/svelte/icons/x";
	import PanelLeftIcon from "@lucide/svelte/icons/panel-left";
	import {
		minimizeWindow,
		toggleMaximizeWindow,
		closeWindow,
	} from "$lib/utils/window";
	import { getFileName } from "$lib/utils/path";

	interface Props {
		onToggleSidebar?: () => void;
		sidebarOpen?: boolean;
	}

	let { onToggleSidebar, sidebarOpen = true }: Props = $props();

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

	async function handleProject() {
		if (isProcessing) return;
		try {
			isProcessing = true;
			await invoke("show_projects_window");
		} catch (err) {
			console.error("Failed to open projects window:", err);
		} finally {
			isProcessing = false;
		}
	}

	async function handleSettings() {
		if (isProcessing) return;
		try {
			isProcessing = true;
			await invoke("show_settings_window");
		} catch (err) {
			console.error("Failed to open settings window:", err);
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
		await closeWindow();
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

	// Derive title from current file path
	let windowTitle = $derived(
		qrateStore.currentFilePath
			? `${getFileName(qrateStore.currentFilePath)} - qRate`
			: "qRate",
	);
</script>

<div
	class="flex h-8 shrink-0 select-none items-center justify-between border-b border-border bg-background"
	data-tauri-drag-region
>
	<div class="flex h-full items-center">
		<!-- Sidebar Toggle Button -->
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-10 rounded-none"
			onclick={onToggleSidebar}
			title={sidebarOpen ? "Hide Sidebar" : "Show Sidebar"}
		>
			<PanelLeftIcon class="size-4" />
		</Button>

		<div class="mx-1 h-4 w-px bg-border"></div>

		<DropdownMenu.Root>
			<DropdownMenu.Trigger
				class="flex h-full items-center bg-transparent px-3 text-[0.8125rem] text-foreground hover:bg-accent"
			>
				File
			</DropdownMenu.Trigger>
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
					onclick={handleProject}
					disabled={isProcessing}
				>
					Open Project Window
					<DropdownMenu.Shortcut>Ctrl+P</DropdownMenu.Shortcut>
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
			<DropdownMenu.Trigger
				class="flex h-full items-center bg-transparent px-3 text-[0.8125rem] text-foreground hover:bg-accent"
			>
				Edit
			</DropdownMenu.Trigger>
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

		<DropdownMenu.Root>
			<DropdownMenu.Trigger
				class="flex h-full items-center bg-transparent px-3 text-[0.8125rem] text-foreground hover:bg-accent"
			>
				View
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="start" class="min-w-40">
				<DropdownMenu.Item onclick={onToggleSidebar}>
					{sidebarOpen ? "Hide Sidebar" : "Show Sidebar"}
					<DropdownMenu.Shortcut>Ctrl+B</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Separator />
				<DropdownMenu.Item onclick={handleSettings}>
					Settings
					<DropdownMenu.Shortcut>Ctrl+,</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	</div>

	<div
		class="pointer-events-none absolute left-1/2 -translate-x-1/2 text-xs text-muted-foreground"
		data-tauri-drag-region
	>
		{windowTitle}
	</div>

	<div class="flex h-full">
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-[46px] rounded-none"
			onclick={minimizeWindow}
			title="Minimize"
		>
			<MinusIcon class="size-4" />
		</Button>
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-[46px] rounded-none"
			onclick={toggleMaximizeWindow}
			title="Maximize"
		>
			<SquareIcon class="size-3.5" />
		</Button>
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-[46px] rounded-none hover:bg-[#e81123] hover:text-white"
			onclick={closeWindow}
			title="Close"
		>
			<XIcon class="size-4" />
		</Button>
	</div>
</div>
