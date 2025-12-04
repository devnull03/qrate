<script lang="ts">
	import CommandIcon from "@lucide/svelte/icons/command";
	import ModeToggle from "$lib/components/ModeToggle.svelte";
</script>

<div class="flex items-center justify-between">
	<div class="flex items-center gap-4">
		<div
			class="flex size-12 items-center justify-center rounded-xl bg-primary text-primary-foreground"
		>
			<CommandIcon class="size-6" />
		</div>
		<div>
			<h1 class="text-2xl font-bold">qRate</h1>
			<p class="text-sm text-muted-foreground">
				Digital Archival Workspace
			</p>
		</div>
	</div>
	<ModeToggle />
</div>
