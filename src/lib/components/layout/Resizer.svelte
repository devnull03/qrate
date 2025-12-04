<script lang="ts">
	interface Props {
		direction: "horizontal" | "vertical";
		onResize: (delta: number) => void;
		minSize?: number;
		maxSize?: number;
	}

	let {
		direction,
		onResize,
		minSize = 0,
		maxSize = Infinity,
	}: Props = $props();

	let isDragging = $state(false);
	let startPos = $state(0);

	const handleMouseDown = (e: MouseEvent) => {
		e.preventDefault();
		isDragging = true;
		startPos = direction === "horizontal" ? e.clientX : e.clientY;

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
		startPos = currentPos;

		onResize(delta);
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
		e.preventDefault();
		isDragging = true;
		const touch = e.touches[0];
		startPos = direction === "horizontal" ? touch.clientX : touch.clientY;

		document.addEventListener("touchmove", handleTouchMove, {
			passive: false,
		});
		document.addEventListener("touchend", handleTouchEnd);
	};

	const handleTouchMove = (e: TouchEvent) => {
		if (!isDragging || e.touches.length !== 1) return;
		const touch = e.touches[0];
		const currentPos =
			direction === "horizontal" ? touch.clientX : touch.clientY;
		const delta = currentPos - startPos;
		startPos = currentPos;

		onResize(delta);
		e.preventDefault();
	};

	const handleTouchEnd = () => {
		isDragging = false;
		document.removeEventListener("touchmove", handleTouchMove);
		document.removeEventListener("touchend", handleTouchEnd);
	};

	// Keyboard support for accessibility
	const handleKeyDown = (e: KeyboardEvent) => {
		const step = e.shiftKey ? 10 : 1;

		if (direction === "horizontal") {
			if (e.key === "ArrowLeft") {
				e.preventDefault();
				onResize(-step);
			} else if (e.key === "ArrowRight") {
				e.preventDefault();
				onResize(step);
			}
		} else {
			if (e.key === "ArrowUp") {
				e.preventDefault();
				onResize(-step);
			} else if (e.key === "ArrowDown") {
				e.preventDefault();
				onResize(step);
			}
		}
	};
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
	class="resizer {direction}"
	class:is-dragging={isDragging}
	onmousedown={handleMouseDown}
	ontouchstart={handleTouchStart}
	onkeydown={handleKeyDown}
	tabindex="0"
	role="separator"
	aria-orientation={direction === "horizontal" ? "vertical" : "horizontal"}
	aria-label="Resize {direction === 'horizontal' ? 'sidebar' : 'panel'}"
	aria-valuenow={50}
></div>

<style>
	.resizer {
		position: relative;
		background: transparent;
		transition: background-color 0.15s ease;
		z-index: 20;
		flex-shrink: 0;
	}

	.resizer.horizontal {
		width: 4px;
		cursor: col-resize;
		margin: 0 -2px;
	}

	.resizer.vertical {
		height: 4px;
		cursor: row-resize;
		margin: -2px 0;
	}

	.resizer:hover,
	.resizer.is-dragging {
		background: hsl(var(--primary) / 0.5);
	}

	.resizer:focus-visible {
		outline: 2px solid hsl(var(--ring));
		outline-offset: -2px;
		background: hsl(var(--primary) / 0.3);
	}

	/* Larger hit area */
	.resizer::before {
		content: "";
		position: absolute;
	}

	.resizer.horizontal::before {
		top: 0;
		bottom: 0;
		left: -4px;
		right: -4px;
	}

	.resizer.vertical::before {
		left: 0;
		right: 0;
		top: -4px;
		bottom: -4px;
	}
</style>
