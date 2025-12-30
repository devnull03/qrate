<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import type { Snippet } from "svelte";

	interface Props {
		children: Snippet;
	}

	let { children }: Props = $props();
</script>

{#if !qrateStore.isFileOpen}
	<div class="flex h-full w-full items-center justify-center">
		<div class="text-center text-muted-foreground">
			<p class="mb-2 text-lg">No project open</p>
			<p class="text-sm">
				Open a project folder or import a CSV to get started
			</p>
		</div>
	</div>
{:else}
	<div class="relative min-h-0 h-full">
		{@render children()}

		{#if qrateStore.isLoading}
			<div
				class="absolute inset-0 flex items-center justify-center bg-background/80 backdrop-blur-sm"
			>
				<div
					class="flex items-center gap-2 text-sm text-muted-foreground"
				>
					<div
						class="size-4 animate-spin rounded-full border-2 border-current border-t-transparent"
					></div>
					<span>Loading...</span>
				</div>
			</div>
		{/if}
	</div>
{/if}

{#if qrateStore.error}
	<div
		class="fixed bottom-4 right-4 z-50 max-w-sm rounded-lg border border-destructive/50 bg-destructive/10 p-4 shadow-lg backdrop-blur-sm"
	>
		<p class="font-semibold text-destructive">Error</p>
		<p class="text-sm text-destructive/90">{qrateStore.error}</p>
		<Button
			variant="ghost"
			size="sm"
			class="mt-2 h-auto p-0 text-xs text-destructive underline hover:bg-transparent"
			onclick={() => (qrateStore.error = null)}
		>
			Dismiss
		</Button>
	</div>
{/if}
