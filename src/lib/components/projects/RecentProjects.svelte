<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import FileIcon from "@lucide/svelte/icons/file";
	import ClockIcon from "@lucide/svelte/icons/clock";
	import XIcon from "@lucide/svelte/icons/x";
	import type { RecentFile } from "$lib/stores/recentFiles";

	interface Props {
		recentFiles: RecentFile[];
		isProcessing: boolean;
		onOpenRecent: (path: string) => void;
		onRemoveRecent: (path: string) => Promise<void>;
	}

	let { recentFiles, isProcessing, onOpenRecent, onRemoveRecent }: Props =
		$props();

	function formatRelativeTime(timestamp: number): string {
		const diff = Date.now() - timestamp;
		const minutes = Math.floor(diff / 60000);
		const hours = Math.floor(minutes / 60);
		const days = Math.floor(hours / 24);

		if (days > 0) return `${days} day${days > 1 ? "s" : ""} ago`;
		if (hours > 0) return `${hours} hour${hours > 1 ? "s" : ""} ago`;
		if (minutes > 0)
			return `${minutes} minute${minutes > 1 ? "s" : ""} ago`;
		return "Just now";
	}
</script>

{#if recentFiles.length > 0}
	<Card.Root>
		<Card.Header>
			<Card.Title class="flex items-center gap-2">
				<ClockIcon class="size-4" />
				Recent Projects
			</Card.Title>
		</Card.Header>
		<Card.Content class="space-y-2">
			{#each recentFiles as file (file.path)}
				<div
					class="group flex w-full items-center gap-3 rounded-md p-3 text-left transition-colors hover:bg-muted"
				>
					<button
						onclick={() => onOpenRecent(file.path)}
						disabled={isProcessing}
						class="flex min-w-0 flex-1 items-center gap-3 disabled:opacity-50"
					>
						<FileIcon
							class="size-5 shrink-0 text-muted-foreground"
						/>
						<div class="min-w-0 flex-1">
							<p class="truncate text-left font-medium">
								{file.name}
							</p>
							<p
								class="truncate text-left text-xs text-muted-foreground"
								title={file.path}
							>
								{file.path}
							</p>
						</div>
						<span class="shrink-0 text-xs text-muted-foreground">
							{formatRelativeTime(file.lastOpened)}
						</span>
					</button>
					<Button
						variant="ghost"
						size="icon-sm"
						class="shrink-0 opacity-0 group-hover:opacity-100"
						onclick={() => onRemoveRecent(file.path)}
						title="Remove from recent"
					>
						<XIcon class="size-4" />
					</Button>
				</div>
			{/each}
		</Card.Content>
	</Card.Root>
{/if}
