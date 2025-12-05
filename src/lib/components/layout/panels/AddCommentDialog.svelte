<script lang="ts">
	import * as Popover from "$lib/components/ui/popover/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { annotationsService } from "$lib/services/annotations";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";

	interface Props {
		open: boolean;
		rowId: number | null;
		columnId: string | null;
		onClose: () => void;
		anchorPosition: { x: number; y: number };
	}

	let {
		open = $bindable(),
		rowId,
		columnId,
		onClose,
		anchorPosition,
	}: Props = $props();

	let message = $state("");
	let isSubmitting = $state(false);
	let error = $state<string | null>(null);
	let textareaRef = $state<HTMLTextAreaElement | null>(null);

	const locationLabel = $derived.by(() => {
		if (rowId !== null && columnId !== null) {
			const column = qrateStore.columns.find((c) => c.id === columnId);
			const columnName = column?.name ?? columnId;
			return `Row ${rowId}, ${columnName}`;
		}
		if (rowId !== null) {
			return `Row ${rowId}`;
		}
		if (columnId !== null) {
			const column = qrateStore.columns.find((c) => c.id === columnId);
			return column?.name ?? columnId;
		}
		return "Unknown location";
	});

	$effect(() => {
		if (open && textareaRef) {
			setTimeout(() => textareaRef?.focus(), 0);
		}
	});

	async function handleSubmit() {
		if (!message.trim()) {
			error = "Please enter a comment";
			return;
		}

		isSubmitting = true;
		error = null;

		try {
			await annotationsService.add({
				provider: "user-comment",
				rowId,
				columnId,
				message: message.trim(),
			});

			message = "";
			open = false;
			onClose();
		} catch (err) {
			error = err instanceof Error ? err.message : String(err);
		} finally {
			isSubmitting = false;
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
			e.preventDefault();
			handleSubmit();
		}
		if (e.key === "Escape") {
			e.preventDefault();
			handleOpenChange(false);
		}
	}

	function handleOpenChange(isOpen: boolean) {
		if (!isOpen) {
			message = "";
			error = null;
			onClose();
		}
		open = isOpen;
	}
</script>

<div
	style="position: fixed; left: {anchorPosition.x}px; top: {anchorPosition.y}px; width: 0; height: 0; pointer-events: none;"
>
	<Popover.Root bind:open onOpenChange={handleOpenChange}>
		<Popover.Trigger class="sr-only" aria-hidden="true"
			>anchor</Popover.Trigger
		>
		<Popover.Content
			class="w-80"
			align="start"
			side="bottom"
			sideOffset={8}
		>
			<div class="flex flex-col gap-3">
				<div class="space-y-1">
					<h4 class="text-sm font-medium leading-none">
						Add Comment
					</h4>
					<p class="text-xs text-muted-foreground">{locationLabel}</p>
				</div>

				<textarea
					bind:this={textareaRef}
					bind:value={message}
					onkeydown={handleKeydown}
					placeholder="Enter your comment..."
					class="min-h-[80px] w-full resize-none rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
					disabled={isSubmitting}
				></textarea>

				{#if error}
					<p class="text-xs text-destructive">{error}</p>
				{/if}

				<div class="flex items-center justify-between">
					<p class="text-xs text-muted-foreground">
						Ctrl+Enter to submit
					</p>
					<div class="flex gap-2">
						<Button
							variant="ghost"
							size="sm"
							onclick={() => handleOpenChange(false)}
							disabled={isSubmitting}
						>
							Cancel
						</Button>
						<Button
							size="sm"
							onclick={handleSubmit}
							disabled={isSubmitting || !message.trim()}
						>
							{#if isSubmitting}
								Adding...
							{:else}
								Add
							{/if}
						</Button>
					</div>
				</div>
			</div>
		</Popover.Content>
	</Popover.Root>
</div>
