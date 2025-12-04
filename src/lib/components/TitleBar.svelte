<script lang="ts">
	import { invoke } from "@tauri-apps/api/core";
	import { open, save } from "@tauri-apps/plugin-dialog";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import { layoutStore } from "$lib/stores/layoutStore.svelte";
	import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import MinusIcon from "@lucide/svelte/icons/minus";
	import SquareIcon from "@lucide/svelte/icons/square";
	import XIcon from "@lucide/svelte/icons/x";
	import PanelLeftIcon from "@lucide/svelte/icons/panel-left";
	import PanelRightIcon from "@lucide/svelte/icons/panel-right";
	import PanelBottomIcon from "@lucide/svelte/icons/panel-bottom";
	import {
		minimizeWindow,
		toggleMaximizeWindow,
		closeWindow,
	} from "$lib/utils/window";
	import { getFileName } from "$lib/utils/path";

	let isProcessing = $state(false);

	// Panel visibility states
	let leftSidebarVisible = $derived(
		layoutStore.layout?.left_sidebar?.visible ?? false,
	);
	let rightSidebarVisible = $derived(
		layoutStore.layout?.right_sidebar?.visible ?? true,
	);
	let bottomPanelVisible = $derived(
		layoutStore.layout?.bottom_panel?.visible ?? false,
	);

	// Panel toggle actions
	async function toggleLeftSidebar() {
		await layoutStore.toggleRegion("left_sidebar");
	}

	async function toggleRightSidebar() {
		await layoutStore.toggleRegion("right_sidebar");
	}

	async function toggleBottomPanel() {
		await layoutStore.toggleRegion("bottom_panel");
	}

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
				<DropdownMenu.Item onclick={toggleLeftSidebar}>
					{leftSidebarVisible ? "Hide" : "Show"} Left Sidebar
					<DropdownMenu.Shortcut>Ctrl+B</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Item onclick={toggleRightSidebar}>
					{rightSidebarVisible ? "Hide" : "Show"} Right Sidebar
					<DropdownMenu.Shortcut>Ctrl+Alt+B</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Item onclick={toggleBottomPanel}>
					{bottomPanelVisible ? "Hide" : "Show"} Bottom Panel
					<DropdownMenu.Shortcut>Ctrl+`</DropdownMenu.Shortcut>
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
		class="pointer-events-none text-xs text-muted-foreground text-ellipsis whitespace-nowrap max-w-[calc(100%-14rem)]min-w-0 overflow-hidden sm:max-w-[200px] md:max-w-[400px] lg:max-w-[600px] xl:max-w-[800px] 2xl:max-w-[1000px]"
		data-tauri-drag-region
	>
		{windowTitle}
	</div>

	<div class="flex h-full items-center">
		<!-- Panel Toggle Buttons -->
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-7 rounded-none {leftSidebarVisible
				? 'bg-accent/50'
				: ''}"
			onclick={toggleLeftSidebar}
			title={leftSidebarVisible
				? "Hide Left Sidebar (Ctrl+B)"
				: "Show Left Sidebar (Ctrl+B)"}
		>
			<PanelLeftIcon class="size-4" />
		</Button>
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-7 rounded-none {bottomPanelVisible
				? 'bg-accent/50'
				: ''}"
			onclick={toggleBottomPanel}
			title={bottomPanelVisible
				? "Hide Bottom Panel (Ctrl+`)"
				: "Show Bottom Panel (Ctrl+`)"}
		>
			<PanelBottomIcon class="size-4" />
		</Button>
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-7 rounded-none {rightSidebarVisible
				? 'bg-accent/50'
				: ''}"
			onclick={toggleRightSidebar}
			title={rightSidebarVisible
				? "Hide Right Sidebar (Ctrl+Alt+B)"
				: "Show Right Sidebar (Ctrl+Alt+B)"}
		>
			<PanelRightIcon class="size-4" />
		</Button>

		<div class="mx-1 h-4 w-px bg-border"></div>

		<!-- Window Controls -->
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
