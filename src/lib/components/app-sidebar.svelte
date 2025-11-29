<script lang="ts">
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";
	import type { ComponentProps } from "svelte";

	let {
		ref = $bindable(null),
		...restProps
	}: ComponentProps<typeof Sidebar.Root> = $props();
</script>

<Sidebar.Root bind:ref collapsible="icon" {...restProps}>
	<Sidebar.Header>
		<!-- Future: Mode selector or branding -->
	</Sidebar.Header>
	<Sidebar.Content>
		<!-- Future: Mode-specific content will go here -->
	</Sidebar.Content>
	<Sidebar.Footer>
		<!-- Future: User menu or settings -->
	</Sidebar.Footer>
</Sidebar.Root>
