<script lang="ts">
	import RevoGrid from "$lib/components/grid/RevoGrid.svelte";
	import FilesGrid from "$lib/components/FilesGrid.svelte";
	import RowDetailsPanel from "$lib/components/layout/panels/RowDetailsPanel.svelte";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
	import TableIcon from "@lucide/svelte/icons/table";
	import FolderIcon from "@lucide/svelte/icons/folder";
	import PanelLeftIcon from "@lucide/svelte/icons/panel-left";
	import PanelRightIcon from "@lucide/svelte/icons/panel-right";
	import ColumnsIcon from "@lucide/svelte/icons/columns-2";
	import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import {
		getGlobalSetting,
		setGlobalSetting,
	} from "$lib/stores/globalSettings";
	import { onMount } from "svelte";
	import SvarGrid from "$lib/components/grid/SvarGrid.svelte";
	import GridContainer from "$lib/components/grid/GridContainer.svelte";

	type ViewMode = "spreadsheet" | "files";
	type SplitDirection = "left" | "right";

	let activeView = $state<ViewMode>("spreadsheet");
	let splitDirection = $state<SplitDirection>("right");
	let splitSize = $state(65);
	let isDragging = $state(false);
	let saveTimeout: ReturnType<typeof setTimeout> | null = null;
	let containerRef = $state<HTMLDivElement | null>(null);

	onMount(() => {
		const savedDir = getGlobalSetting("splitDirection");
		if (savedDir === "left" || savedDir === "right")
			splitDirection = savedDir;

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
		const pointerPercent = ((e.clientX - rect.left) / rect.width) * 100;
		splitSize = isLeft
			? Math.max(20, Math.min(80, 100 - pointerPercent))
			: Math.max(20, Math.min(80, pointerPercent));
		debounceSave();
	}

	function handlePointerUp(e: PointerEvent) {
		(e.target as HTMLElement).releasePointerCapture(e.pointerId);
		isDragging = false;
	}

	function handleResizeKeyDown(e: KeyboardEvent) {
		const step = e.shiftKey ? 5 : 1;
		if (e.key === "ArrowLeft") {
			e.preventDefault();
			splitSize = Math.max(20, splitSize - step);
			debounceSave();
		} else if (e.key === "ArrowRight") {
			e.preventDefault();
			splitSize = Math.min(80, splitSize + step);
			debounceSave();
		}
	}

	function debounceSave() {
		if (saveTimeout) clearTimeout(saveTimeout);
		saveTimeout = setTimeout(
			() => setGlobalSetting("splitSize", Math.round(splitSize)),
			300,
		);
	}

	const isOpen = $derived(qrateStore.detailsPanelOpen);
	const isLeft = $derived(splitDirection === "left");
	const detailsSize = $derived(isOpen ? 100 - splitSize : 0);
	const mainLeft = $derived(isOpen && isLeft ? 100 - splitSize : 0);
	const mainRight = $derived(isOpen && !isLeft ? 100 - splitSize : 0);
	const handlePosition = $derived(isLeft ? 100 - splitSize : splitSize);
</script>

<div class="flex h-full flex-col">
	<div
		class="flex items-center border-b border-border bg-muted/30 px-4 py-1.5"
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

			<div class="mx-2 h-4 w-px bg-border"></div>

			<div class="flex items-center">
				<Button
					variant={isOpen ? "secondary" : "ghost"}
					size="sm"
					class="h-7 gap-1.5 rounded-r-none px-3"
					onclick={() => qrateStore.toggleDetailsPanel()}
					title="Toggle Details (Ctrl+K)"
				>
					<ColumnsIcon class="size-3.5" />
					<span>Details</span>
				</Button>
				<DropdownMenu.Root>
					<DropdownMenu.Trigger>
						{#snippet child({ props })}
							<Button
								{...props}
								variant={isOpen ? "secondary" : "ghost"}
								size="sm"
								class="h-7 rounded-l-none px-1"
							>
								<ChevronDownIcon class="size-3.5" />
							</Button>
						{/snippet}
					</DropdownMenu.Trigger>
					<DropdownMenu.Content align="start" class="w-40">
						<DropdownMenu.RadioGroup
							value={splitDirection}
							onValueChange={handleDirectionChange}
						>
							<DropdownMenu.RadioItem value="left" class="gap-2">
								<PanelLeftIcon class="size-4" />
								Show Left
							</DropdownMenu.RadioItem>
							<DropdownMenu.RadioItem value="right" class="gap-2">
								<PanelRightIcon class="size-4" />
								Show Right
							</DropdownMenu.RadioItem>
						</DropdownMenu.RadioGroup>
					</DropdownMenu.Content>
				</DropdownMenu.Root>
			</div>
		</div>
	</div>

	<div
		bind:this={containerRef}
		class="relative min-h-0"
		class:select-none={isDragging}
	>
		{#if isLeft && isOpen}
			<div
				class="absolute bottom-0 left-0 top-0 overflow-hidden border-r border-border"
				class:pointer-events-none={isDragging}
				style="width: {detailsSize}%;"
			>
				<RowDetailsPanel />
			</div>
		{/if}

		<div
			class="absolute bottom-0 top-0"
			class:pointer-events-none={isDragging}
			style="left: {mainLeft}%; right: {mainRight}%;"
		>
			<div class="h-full" class:hidden={activeView !== "spreadsheet"}>
				<!-- <RevoGrid /> -->
				<GridContainer>
					<SvarGrid />
				</GridContainer>
			</div>
			<div class="h-full" class:hidden={activeView !== "files"}>
				<FilesGrid />
			</div>
		</div>

		{#if !isLeft && isOpen}
			<div
				class="absolute bottom-0 right-0 top-0 overflow-hidden border-l border-border"
				class:pointer-events-none={isDragging}
				style="width: {detailsSize}%;"
			>
				<RowDetailsPanel />
			</div>
		{/if}

		{#if isOpen}
			<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
			<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
			<div
				class="absolute bottom-0 top-0 z-10 w-1 -translate-x-1/2 cursor-col-resize touch-none transition-colors duration-150 hover:bg-primary/50 {isDragging
					? 'bg-primary/50'
					: ''}"
				style="left: {handlePosition}%;"
				onpointerdown={handlePointerDown}
				onpointermove={handlePointerMove}
				onpointerup={handlePointerUp}
				onpointercancel={handlePointerUp}
				onkeydown={handleResizeKeyDown}
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
