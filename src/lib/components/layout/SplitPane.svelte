<script lang="ts">
	import type { Snippet } from "svelte";

	interface Props {
		direction?: "horizontal" | "vertical";
		defaultSize?: number;
		minSize?: number;
		maxSize?: number;
		first: Snippet;
		second: Snippet;
	}

	let {
		direction = "horizontal",
		defaultSize = 50,
		minSize = 20,
		maxSize = 80,
		first,
		second,
	}: Props = $props();

	let splitSize = $state(defaultSize);
	let isDragging = $state(false);
	let containerRef = $state<HTMLDivElement | null>(null);

	function calculateSize(clientX: number, clientY: number) {
		if (!containerRef) return;
		const rect = containerRef.getBoundingClientRect();
		const pos =
			direction === "horizontal"
				? clientX - rect.left
				: clientY - rect.top;
		const total = direction === "horizontal" ? rect.width : rect.height;
		splitSize = Math.max(minSize, Math.min(maxSize, (pos / total) * 100));
	}

	function handlePointerDown(e: PointerEvent) {
		e.preventDefault();
		(e.target as HTMLElement).setPointerCapture(e.pointerId);
		isDragging = true;
	}

	function handlePointerMove(e: PointerEvent) {
		if (!isDragging) return;
		calculateSize(e.clientX, e.clientY);
	}

	function handlePointerUp(e: PointerEvent) {
		(e.target as HTMLElement).releasePointerCapture(e.pointerId);
		isDragging = false;
	}

	function handleKeyDown(e: KeyboardEvent) {
		const step = e.shiftKey ? 5 : 1;
		const keys =
			direction === "horizontal"
				? { decrease: "ArrowLeft", increase: "ArrowRight" }
				: { decrease: "ArrowUp", increase: "ArrowDown" };

		if (e.key === keys.decrease) {
			e.preventDefault();
			splitSize = Math.max(minSize, splitSize - step);
		} else if (e.key === keys.increase) {
			e.preventDefault();
			splitSize = Math.min(maxSize, splitSize + step);
		}
	}

	const isHorizontal = $derived(direction === "horizontal");
</script>

<div
	bind:this={containerRef}
	class="flex h-full w-full overflow-hidden {isHorizontal
		? 'flex-row'
		: 'flex-col'}"
	class:select-none={isDragging}
>
	<div
		class="shrink-0 overflow-hidden"
		class:pointer-events-none={isDragging}
		style="{isHorizontal ? 'width' : 'height'}: {splitSize}%;"
	>
		{@render first()}
	</div>

	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<div
		class="relative z-10 shrink-0 touch-none transition-colors duration-150
			{isHorizontal ? 'w-1 cursor-col-resize' : 'h-1 cursor-row-resize'}
			{isDragging ? 'bg-primary/50' : 'hover:bg-primary/50'}"
		onpointerdown={handlePointerDown}
		onpointermove={handlePointerMove}
		onpointerup={handlePointerUp}
		onpointercancel={handlePointerUp}
		onkeydown={handleKeyDown}
		tabindex="0"
		role="separator"
		aria-orientation={isHorizontal ? "vertical" : "horizontal"}
		aria-valuenow={Math.round(splitSize)}
		aria-valuemin={minSize}
		aria-valuemax={maxSize}
	></div>

	<div
		class="min-h-0 min-w-0 flex-1 overflow-hidden"
		class:pointer-events-none={isDragging}
	>
		{@render second()}
	</div>
</div>
