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
	import ColumnsIcon from "@lucide/svelte/icons/columns-2";
	import {
		minimizeWindow,
		toggleMaximizeWindow,
		closeWindow,
	} from "$lib/utils/window";
	import { getFileName } from "$lib/utils/path";

	let isProcessing = $state(false);

	const leftVisible = $derived(
		layoutStore.layout?.left_sidebar?.visible ?? false,
	);
	const rightVisible = $derived(
		layoutStore.layout?.right_sidebar?.visible ?? true,
	);
	const bottomVisible = $derived(
		layoutStore.layout?.bottom_panel?.visible ?? false,
	);
	const detailsVisible = $derived(qrateStore.detailsPanelOpen);
	const windowTitle = $derived(
		qrateStore.currentFilePath
			? `${getFileName(qrateStore.currentFilePath)} - qRate`
			: "qRate",
	);

	const toggleLeft = () => layoutStore.toggleRegion("left_sidebar");
	const toggleRight = () => layoutStore.toggleRegion("right_sidebar");
	const toggleBottom = () => layoutStore.toggleRegion("bottom_panel");
	const toggleDetails = () => qrateStore.toggleDetailsPanel();

	async function handleNew() {
		if (isProcessing) return;
		isProcessing = true;
		const selected = await save({
			filters: [{ name: "Qrate Files", extensions: ["qrate"] }],
			defaultPath: "untitled.qrate",
		}).catch(() => null);
		if (selected) await qrateStore.createFile(selected).catch(() => {});
		isProcessing = false;
	}

	async function handleOpen() {
		if (isProcessing) return;
		isProcessing = true;
		const selected = await open({
			multiple: false,
			filters: [{ name: "Qrate Files", extensions: ["qrate"] }],
		}).catch(() => null);
		if (selected && typeof selected === "string")
			await qrateStore.openFile(selected).catch(() => {});
		isProcessing = false;
	}

	async function handleProject() {
		if (isProcessing) return;
		isProcessing = true;
		await invoke("show_projects_window").catch(() => {});
		isProcessing = false;
	}

	async function handleSettings() {
		if (isProcessing) return;
		isProcessing = true;
		await invoke("show_settings_window").catch(() => {});
		isProcessing = false;
	}

	async function handleImportCsv() {
		if (isProcessing) return;
		isProcessing = true;
		const csvFile = await open({
			multiple: false,
			filters: [{ name: "CSV Files", extensions: ["csv"] }],
		}).catch(() => null);
		if (csvFile && typeof csvFile === "string") {
			const qrateFile = await save({
				filters: [{ name: "Qrate Files", extensions: ["qrate"] }],
				defaultPath: csvFile.replace(/\.csv$/i, ".qrate"),
			}).catch(() => null);
			if (qrateFile)
				await qrateStore.importCsv(qrateFile, csvFile).catch(() => {});
		}
		isProcessing = false;
	}

	const handleClose = () => qrateStore.closeFile().catch(() => {});
	const handleQuit = () => closeWindow();
	const handleUndo = () => document.execCommand("undo");
	const handleRedo = () => document.execCommand("redo");
	const handleCut = () => document.execCommand("cut");
	const handleCopy = () => document.execCommand("copy");
	const handlePaste = () => document.execCommand("paste");
	const handleSelectAll = () => document.execCommand("selectAll");
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
				<DropdownMenu.Item disabled={!qrateStore.isFileOpen}>
					Save
					<DropdownMenu.Shortcut>Ctrl+S</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Item disabled={!qrateStore.isFileOpen}>
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
				<DropdownMenu.Item onclick={toggleLeft}>
					{leftVisible ? "Hide" : "Show"} Left Sidebar
					<DropdownMenu.Shortcut>Ctrl+B</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Item onclick={toggleRight}>
					{rightVisible ? "Hide" : "Show"} Right Sidebar
					<DropdownMenu.Shortcut>Ctrl+Alt+B</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Item onclick={toggleBottom}>
					{bottomVisible ? "Hide" : "Show"} Bottom Panel
					<DropdownMenu.Shortcut>Ctrl+`</DropdownMenu.Shortcut>
				</DropdownMenu.Item>
				<DropdownMenu.Item onclick={toggleDetails}>
					{detailsVisible ? "Hide" : "Show"} Details Panel
					<DropdownMenu.Shortcut>Ctrl+L</DropdownMenu.Shortcut>
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
		class="pointer-events-none min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-xs text-muted-foreground"
		data-tauri-drag-region
	>
		{windowTitle}
	</div>

	<div class="flex h-full items-center">
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-7 rounded-none {leftVisible ? 'bg-accent/50' : ''}"
			onclick={toggleLeft}
			title={leftVisible
				? "Hide Left Sidebar (Ctrl+B)"
				: "Show Left Sidebar (Ctrl+B)"}
		>
			<PanelLeftIcon class="size-4" />
		</Button>
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-7 rounded-none {bottomVisible
				? 'bg-accent/50'
				: ''}"
			onclick={toggleBottom}
			title={bottomVisible
				? "Hide Bottom Panel (Ctrl+`)"
				: "Show Bottom Panel (Ctrl+`)"}
		>
			<PanelBottomIcon class="size-4" />
		</Button>
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-7 rounded-none {rightVisible ? 'bg-accent/50' : ''}"
			onclick={toggleRight}
			title={rightVisible
				? "Hide Right Sidebar (Ctrl+Alt+B)"
				: "Show Right Sidebar (Ctrl+Alt+B)"}
		>
			<PanelRightIcon class="size-4" />
		</Button>
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-7 rounded-none {detailsVisible
				? 'bg-accent/50'
				: ''}"
			onclick={toggleDetails}
			title={detailsVisible
				? "Hide Details Panel (Ctrl+L)"
				: "Show Details Panel (Ctrl+L)"}
		>
			<ColumnsIcon class="size-4" />
		</Button>

		<div class="mx-1 h-4 w-px bg-border"></div>

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
