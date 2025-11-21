<script lang="ts">
	import * as Breadcrumb from "$lib/components/ui/breadcrumb/index.js";
	import { Separator } from "$lib/components/ui/separator/index.js";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";
	import ModeToggle from "$lib/components/ModeToggle.svelte";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";

	// Get current file name for breadcrumb
	let currentFileName = $derived.by(() => {
		if (!qrateStore.currentFilePath) return null;
		const parts = qrateStore.currentFilePath.split(/[\\/]/);
		return parts[parts.length - 1];
	});
</script>

<header class="header-fixed">
	<Sidebar.Trigger class="-ml-1" />
	<Separator
		orientation="vertical"
		class="mr-2 data-[orientation=vertical]:h-4"
	/>
	<Breadcrumb.Root>
		<Breadcrumb.List>
			<Breadcrumb.Item class="hidden md:block">
				<Breadcrumb.Link href="/">qRate</Breadcrumb.Link
				>
			</Breadcrumb.Item>
			{#if currentFileName}
				<Breadcrumb.Separator class="hidden md:block" />
				<Breadcrumb.Item>
					<Breadcrumb.Page
						>{currentFileName}</Breadcrumb.Page
					>
				</Breadcrumb.Item>
			{:else}
				<Breadcrumb.Separator class="hidden md:block" />
				<Breadcrumb.Item>
					<Breadcrumb.Page
						>No file open</Breadcrumb.Page
					>
				</Breadcrumb.Item>
			{/if}
		</Breadcrumb.List>
	</Breadcrumb.Root>

	<div class="ml-auto">
		<ModeToggle />
	</div>
</header>

<style>
	.header-fixed {
		background-color: hsl(var(--background));
		position: sticky;
		top: 0;
		display: flex;
		flex-shrink: 0;
		align-items: center;
		gap: 0.5rem;
		border-bottom: 1px solid hsl(var(--border));
		padding: 1rem;
		width: 100%;
		z-index: 10;
		overflow: hidden;
	}
</style>
