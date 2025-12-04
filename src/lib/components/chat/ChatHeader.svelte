<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import MessageSquareIcon from "@lucide/svelte/icons/message-square";
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

		<Button
			variant="ghost"
			size="icon-sm"
			class="size-7"
			onclick={ _ => handleModeChange("Detached")}
			title="Popout"
			disabled
		>
			<ExternalLinkIcon class="size-4" />
		</Button>
</div>

