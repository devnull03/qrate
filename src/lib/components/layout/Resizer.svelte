<script lang="ts">
	interface Props {
		direction: "horizontal" | "vertical";
		onResize: (delta: number) => void;
		minSize?: number;
		maxSize?: number;
	}

	let { direction, onResize, minSize = 0, maxSize = Infinity }: Props = $props();

	let isDragging = $state(false);
	let startPos = $state(0);
	let startSize = $state(0);

	const handleMouseDown = (e: MouseEvent) => {
		isDragging = true;
		startPos = direction === "horizontal" ? e.clientX : e.clientY;
		startSize = 0; // Will be set by parent

		document.addEventListener("mousemove", handleMouseMove);
		document.addEventListener("mouseup", handleMouseUp);
		document.body.style.cursor =
			direction === "horizontal" ? "col-resize" : "row-resize";
		document.body.style.userSelect = "none";
	};

	const handleMouseMove = (e: MouseEvent) => {
		if (!isDragging) return;

		const currentPos = direction === "horizontal" ? e.clientX : e.clientY;
		const delta = currentPos - startPos;

		// Apply constraints
		const constrainedDelta = Math.max(
			minSize - startSize,
			Math.min(maxSize - startSize, delta),
		);

		onResize(constrainedDelta);
	};

	const handleMouseUp = () => {
		isDragging = false;
		document.removeEventListener("mousemove", handleMouseMove);
		document.removeEventListener("mouseup", handleMouseUp);
		document.body.style.cursor = "";
		document.body.style.userSelect = "";
	};

	// Touch support
	const handleTouchStart = (e: TouchEvent) => {
		if (e.touches.length !== 1) return;
		isDragging = true;
		const touch = e.touches[0];
		startPos = direction === "horizontal" ? touch.clientX : touch.clientY;
		startSize = 0;

		document.addEventListener("touchmove", handleTouchMove, { passive: false });
		document.addEventListener("touchend", handleTouchEnd);
		e.preventDefault();
	};

	const handleTouchMove = (e: TouchEvent) => {
		if (!isDragging || e.touches.length !== 1) return;
		const touch = e.touches[0];
		const currentPos = direction === "horizontal" ? touch.clientX : touch.clientY;
		const delta = currentPos - startPos;

		const constrainedDelta = Math.max(
			minSize - startSize,
			Math.min(maxSize - startSize, delta),
		);

		onResize(constrainedDelta);
		e.preventDefault();
	};

	const handleTouchEnd = () => {
		isDragging = false;
		document.removeEventListener("touchmove", handleTouchMove);
		document.removeEventListener("touchend", handleTouchEnd);
	};
</script>

<button
	type="button"
	class="resizer {direction}"
	class:is-dragging={isDragging}
	onmousedown={handleMouseDown}
	ontouchstart={handleTouchStart}
	role="separator"
	aria-orientation={direction === "horizontal" ? "vertical" : "horizontal"}
	aria-label="Resize {direction === 'horizontal' ? 'sidebar' : 'panel'}"
	tabindex="0"
></button>

<style>
	.resizer {
		position: relative;
		background: transparent;
		transition: background-color 0.15s ease;
		z-index: 10;
	}

	.resizer.horizontal {
		width: 4px;
		cursor: col-resize;
		height: 100%;
	}

	.resizer.vertical {
		height: 4px;
		cursor: row-resize;
		width: 100%;
	}

	.resizer:hover,
	.resizer.is-dragging {
		background: hsl(var(--primary) / 0.5);
	}

	.resizer:focus-visible {
		outline: 2px solid hsl(var(--ring));
		outline-offset: -2px;
	}
</style>

