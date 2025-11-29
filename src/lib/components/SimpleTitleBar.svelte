<script lang="ts">
	import { getCurrentWindow } from "@tauri-apps/api/window";
	import MinusIcon from "@lucide/svelte/icons/minus";
	import SquareIcon from "@lucide/svelte/icons/square";
	import XIcon from "@lucide/svelte/icons/x";

	interface Props {
		title?: string;
	}

	let { title = "qRate" }: Props = $props();

	const appWindow = getCurrentWindow();

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
	<div class="titlebar-title" data-tauri-drag-region>
		{title}
	</div>

	<div class="titlebar-controls">
		<button class="control-button" onclick={minimize} title="Minimize">
			<MinusIcon class="size-4" />
		</button>
		<button class="control-button" onclick={toggleMaximize} title="Maximize">
			<SquareIcon class="size-3.5" />
		</button>
		<button class="control-button control-close" onclick={close} title="Close">
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

	.titlebar-title {
		flex: 1;
		padding-left: 0.75rem;
		font-size: 0.8125rem;
		color: var(--foreground);
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
