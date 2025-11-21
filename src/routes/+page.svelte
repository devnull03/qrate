<script lang="ts">
	import RevoGrid from "$lib/grid/RevoGrid.svelte";
	import FileManager from "$lib/components/FileManager.svelte";
</script>

<div class="flex h-[calc(100vh-73px)] w-full">
	<FileManager />
	<div class="flex-1 p-4">
		<RevoGrid />
	</div>
</div>
