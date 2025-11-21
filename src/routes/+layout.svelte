<script lang="ts">
	import "../app.css";

	import AppSidebar from "$lib/components/app-sidebar.svelte";
	import Header from "$lib/components/Header.svelte";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";

	import { ModeWatcher } from "mode-watcher";

	let { children } = $props();
</script>

<Sidebar.Provider style="--sidebar-width: 350px;">
	<AppSidebar />
	<Sidebar.Inset>
		<Header />

		<ModeWatcher />
		{@render children()}
	</Sidebar.Inset>
</Sidebar.Provider>
