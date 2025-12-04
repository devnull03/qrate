<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import UploadIcon from "@lucide/svelte/icons/upload";

	interface Props {
		isProcessing: boolean;
		onNewProject: () => void;
		onOpenProject: () => void;
	}

	let { isProcessing, onNewProject, onOpenProject }: Props = $props();
</script>

<Card.Root>
	<Card.Header>
		<Card.Title>Get Started</Card.Title>
		<Card.Description>
			Create a new project or open an existing one
		</Card.Description>
	</Card.Header>
	<Card.Content>
		<div class="grid grid-cols-2 gap-4">
			<Button
				onclick={onNewProject}
				disabled={isProcessing}
				variant="outline"
				class="flex h-24 flex-col items-center justify-center gap-2"
			>
				<UploadIcon class="size-6" />
				<span>New Project</span>
			</Button>
			<Button
				onclick={onOpenProject}
				disabled={isProcessing}
				variant="outline"
				class="flex h-24 flex-col items-center justify-center gap-2"
			>
				<FolderOpenIcon class="size-6" />
				<span>Open Project</span>
			</Button>
		</div>
	</Card.Content>
</Card.Root>
