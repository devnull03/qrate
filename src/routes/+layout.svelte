<script lang="ts">
	import "../app.css";

	import AppSidebar from "$lib/components/app-sidebar.svelte";
	import Header from "$lib/components/Header.svelte";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";

	import { ModeWatcher } from "mode-watcher";

	let { children } = $props();
</script>

<div class="layout-root">
	<Sidebar.Provider style="--sidebar-width: 350px;">
		<AppSidebar />
		<Sidebar.Inset class="layout-inset">
			<Header />
			<ModeWatcher />
			<div class="content-wrapper">
				{@render children()}
			</div>
		</Sidebar.Inset>
	</Sidebar.Provider>
</div>

<style>
	.layout-root {
		width: 100vw;
		height: 100vh;
		overflow: hidden;
		display: flex;
	}

	:global(.layout-inset) {
		display: flex;
		flex-direction: column;
		height: 100vh;
		width: 100%;
		overflow: hidden;
	}

	.content-wrapper {
		flex: 1;
		overflow: hidden;
		position: relative;
		min-height: 0;
	}
</style>
