<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import MinusIcon from "@lucide/svelte/icons/minus";
	import SquareIcon from "@lucide/svelte/icons/square";
	import XIcon from "@lucide/svelte/icons/x";
	import {
		minimizeWindow,
		toggleMaximizeWindow,
		closeWindow,
	} from "$lib/utils/window";

	interface Props {
		title?: string;
	}

	let { title = "qRate" }: Props = $props();
</script>

<div
	class="flex h-8 shrink-0 select-none items-center justify-between border-b border-border bg-background"
	data-tauri-drag-region
>
	<div
		class="flex-1 pl-3 text-[0.8125rem] text-foreground"
		data-tauri-drag-region
	>
		{title}
	</div>

	<div class="flex h-full">
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-[46px] rounded-none"
			onclick={minimizeWindow}
			title="Minimize"
		>
			<MinusIcon class="size-4" />
		</Button>
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-[46px] rounded-none"
			onclick={toggleMaximizeWindow}
			title="Maximize"
		>
			<SquareIcon class="size-3.5" />
		</Button>
		<Button
			variant="ghost"
			size="icon"
			class="h-full w-[46px] rounded-none hover:bg-[#e81123] hover:text-white"
			onclick={closeWindow}
			title="Close"
		>
			<XIcon class="size-4" />
		</Button>
	</div>
</div>
