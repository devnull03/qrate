<script lang="ts">
	import "../app.css";
	import favicon from "$lib/assets/favicon.png";
	import { onNavigate } from "$app/navigation";

	let { children } = $props();
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

{@render children?.()}
