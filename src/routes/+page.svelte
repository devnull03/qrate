<script lang="ts">
	import RevoGrid from "$lib/components/grid/RevoGrid.svelte";
	import FilesGrid from "$lib/components/FilesGrid.svelte";
	import RowDetailsPanel from "$lib/components/RowDetailsPanel.svelte";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
	import TableIcon from "@lucide/svelte/icons/table";
	import FolderIcon from "@lucide/svelte/icons/folder";
	import PanelLeftIcon from "@lucide/svelte/icons/panel-left";
	import PanelRightIcon from "@lucide/svelte/icons/panel-right";
	import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import {
		getGlobalSetting,
		setGlobalSetting,
	} from "$lib/stores/globalSettings";
	import { onMount } from "svelte";

	type ViewMode = "spreadsheet" | "files";
	type SplitDirection = "left" | "right";

	let activeView = $state<ViewMode>("spreadsheet");
	let splitDirection = $state<SplitDirection>("right");
	let isSplitOpen = $state(false);
	let splitSize = $state(65);
	let isDragging = $state(false);
	let saveTimeout: ReturnType<typeof setTimeout> | null = null;
	let containerRef = $state<HTMLDivElement | null>(null);

	onMount(() => {
		const savedDirection = getGlobalSetting("splitDirection");
		if (savedDirection === "left" || savedDirection === "right") {
			splitDirection = savedDirection;
		}
		const savedSize = getGlobalSetting("splitSize");
		if (
			typeof savedSize === "number" &&
			savedSize >= 20 &&
			savedSize <= 80
		) {
			splitSize = savedSize;
		}
	});

	$effect(() => {
		qrateStore.activeView = activeView;
	});

	async function handleDirectionChange(value: string | undefined) {
		if (value === "left" || value === "right") {
			splitDirection = value;
			await setGlobalSetting("splitDirection", value);
		}
	}

	function handlePointerDown(e: PointerEvent) {
		e.preventDefault();
		(e.target as HTMLElement).setPointerCapture(e.pointerId);
		isDragging = true;
	}

	function handlePointerMove(e: PointerEvent) {
		if (!isDragging || !containerRef) return;
		const rect = containerRef.getBoundingClientRect();
		const pos = e.clientX - rect.left;
		splitSize = Math.max(20, Math.min(80, (pos / rect.width) * 100));
		debounceSaveSplitSize();
	}

	function handlePointerUp(e: PointerEvent) {
		(e.target as HTMLElement).releasePointerCapture(e.pointerId);
		isDragging = false;
	}

	function handleKeyDown(e: KeyboardEvent) {
		const step = e.shiftKey ? 5 : 1;
		if (e.key === "ArrowLeft") {
			e.preventDefault();
			splitSize = Math.max(20, splitSize - step);
			debounceSaveSplitSize();
		} else if (e.key === "ArrowRight") {
			e.preventDefault();
			splitSize = Math.min(80, splitSize + step);
			debounceSaveSplitSize();
		}
	}

	function debounceSaveSplitSize() {
		if (saveTimeout) clearTimeout(saveTimeout);
		saveTimeout = setTimeout(() => {
			setGlobalSetting("splitSize", Math.round(splitSize));
		}, 300);
	}

	const mainPanelSize = $derived(
		isSplitOpen
			? splitDirection === "right"
				? splitSize
				: 100 - splitSize
			: 100,
	);
	const detailsPanelSize = $derived(100 - mainPanelSize);
</script>

<div class="flex h-full flex-col overflow-hidden">
	<div
		class="flex items-center justify-between border-b border-border bg-muted/30 px-4 py-1.5"
	>
		<div class="flex items-center gap-1">
			<Button
				variant={activeView === "spreadsheet" ? "secondary" : "ghost"}
				size="sm"
				class="h-7 gap-1.5 px-3"
				onclick={() => (activeView = "spreadsheet")}
			>
				<TableIcon class="size-3.5" />
				<span>Spreadsheet</span>
			</Button>
			<Button
				variant={activeView === "files" ? "secondary" : "ghost"}
				size="sm"
				class="h-7 gap-1.5 px-3"
				onclick={() => (activeView = "files")}
			>
				<FolderIcon class="size-3.5" />
				<span>Files</span>
			</Button>
		</div>

		<div class="flex items-center">
			<Button
				variant={isSplitOpen ? "secondary" : "ghost"}
				size="sm"
				class="h-7 gap-1.5 rounded-r-none border-r-0 px-2"
				onclick={() => (isSplitOpen = !isSplitOpen)}
				title="Toggle split view"
			>
				{#if splitDirection === "right"}
					<PanelRightIcon class="size-3.5" />
				{:else}
					<PanelLeftIcon class="size-3.5" />
				{/if}
				<span class="hidden sm:inline">Split</span>
			</Button>
			<DropdownMenu.Root>
				<DropdownMenu.Trigger>
					{#snippet child({ props })}
						<Button
							{...props}
							variant={isSplitOpen ? "secondary" : "ghost"}
							size="sm"
							class="h-7 rounded-l-none px-1"
						>
							<ChevronDownIcon class="size-3.5" />
						</Button>
					{/snippet}
				</DropdownMenu.Trigger>
				<DropdownMenu.Content align="end" class="w-40">
					<DropdownMenu.RadioGroup
						value={splitDirection}
						onValueChange={handleDirectionChange}
					>
						<DropdownMenu.RadioItem value="left" class="gap-2">
							<PanelLeftIcon class="size-4" />
							Split Left
						</DropdownMenu.RadioItem>
						<DropdownMenu.RadioItem value="right" class="gap-2">
							<PanelRightIcon class="size-4" />
							Split Right
						</DropdownMenu.RadioItem>
					</DropdownMenu.RadioGroup>
				</DropdownMenu.Content>
			</DropdownMenu.Root>
		</div>
	</div>

	<div
		bind:this={containerRef}
		class="relative min-h-0 flex-1 overflow-hidden"
		class:select-none={isDragging}
	>
		{#if splitDirection === "left" && isSplitOpen}
			<div
				class="absolute bottom-0 left-0 top-0 overflow-hidden border-r border-border"
				class:pointer-events-none={isDragging}
				style="width: {detailsPanelSize}%;"
			>
				<RowDetailsPanel />
			</div>
		{/if}

		<div
			class="absolute bottom-0 top-0 overflow-hidden"
			class:pointer-events-none={isDragging}
			style="left: {splitDirection === 'left' && isSplitOpen
				? detailsPanelSize
				: 0}%; right: {splitDirection === 'right' && isSplitOpen
				? detailsPanelSize
				: 0}%;"
		>
			<div class="h-full" class:hidden={activeView !== "spreadsheet"}>
				<RevoGrid />
			</div>
			<div class="h-full" class:hidden={activeView !== "files"}>
				<FilesGrid />
			</div>
		</div>

		{#if splitDirection === "right" && isSplitOpen}
			<div
				class="absolute bottom-0 right-0 top-0 overflow-hidden border-l border-border"
				class:pointer-events-none={isDragging}
				style="width: {detailsPanelSize}%;"
			>
				<RowDetailsPanel />
			</div>
		{/if}

		{#if isSplitOpen}
			<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
			<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
			<div
				class="absolute bottom-0 top-0 z-10 w-1 cursor-col-resize touch-none transition-colors duration-150 hover:bg-primary/50 {isDragging
					? 'bg-primary/50'
					: ''}"
				style="left: {splitSize}%;"
				onpointerdown={handlePointerDown}
				onpointermove={handlePointerMove}
				onpointerup={handlePointerUp}
				onpointercancel={handlePointerUp}
				onkeydown={handleKeyDown}
				tabindex="0"
				role="separator"
				aria-orientation="vertical"
				aria-valuenow={Math.round(splitSize)}
				aria-valuemin={20}
				aria-valuemax={80}
			></div>
		{/if}
	</div>
</div>
