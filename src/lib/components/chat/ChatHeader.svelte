<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
	import MessageSquareIcon from "@lucide/svelte/icons/message-square";
	import XIcon from "@lucide/svelte/icons/x";
	import PanelRightIcon from "@lucide/svelte/icons/panel-right";
	import PanelBottomIcon from "@lucide/svelte/icons/panel-bottom";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import { chatStore, type ChatMode } from "$lib/stores/chatStore.svelte";

	interface Props {
		mode: ChatMode;
		onDetach?: () => void;
		onClose?: () => void;
	}

	let { mode, onDetach, onClose }: Props = $props();

	const modeLabels: Record<ChatMode, string> = {
		Docked: "Docked",
		Panel: "Panel",
		Detached: "Detached",
	};

	async function handleModeChange(newMode: ChatMode) {
		if (newMode === mode) return;

		if (newMode === "Detached") {
			await chatStore.detach();
			onDetach?.();
		} else {
			await chatStore.setMode(newMode);
		}
	}

	async function handleClose() {
		await chatStore.toggleVisible();
		onClose?.();
	}
</script>

<div class="flex items-center justify-between border-b border-border bg-muted/30 px-3 py-2">
	<div class="flex items-center gap-2">
		<MessageSquareIcon class="size-4 text-muted-foreground" />
		<span class="text-sm font-medium">Chat</span>
		<span
			class="rounded-full bg-primary/10 px-2 py-0.5 text-xs text-primary"
		>
			{modeLabels[mode]}
		</span>
	</div>

	<div class="flex items-center gap-1">
		<DropdownMenu.Root>
			<DropdownMenu.Trigger
				class="flex h-7 w-7 items-center justify-center rounded-md hover:bg-accent"
				title="Chat options"
			>
				<PanelRightIcon class="size-3.5" />
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="end" class="w-48">
				<DropdownMenu.Item
					onclick={() => handleModeChange("Docked")}
					disabled={mode === "Docked"}
				>
					<PanelRightIcon class="size-4" />
					<span>Dock to Sidebar</span>
				</DropdownMenu.Item>
				<DropdownMenu.Item
					onclick={() => handleModeChange("Panel")}
					disabled={mode === "Panel"}
				>
					<PanelBottomIcon class="size-4" />
					<span>Move to Panel</span>
				</DropdownMenu.Item>
				<DropdownMenu.Item
					onclick={() => handleModeChange("Detached")}
					disabled={mode === "Detached"}
				>
					<ExternalLinkIcon class="size-4" />
					<span>Detach to Window</span>
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>

		<Button
			variant="ghost"
			size="icon-sm"
			class="size-7"
			onclick={handleClose}
			title="Close chat"
		>
			<XIcon class="size-3.5" />
		</Button>
	</div>
</div>

