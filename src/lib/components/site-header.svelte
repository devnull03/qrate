<script lang="ts">
	import SidebarIcon from "@lucide/svelte/icons/sidebar";
	import SearchForm from "./search-form.svelte";
	import * as Breadcrumb from "$lib/components/ui/breadcrumb/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Separator } from "$lib/components/ui/separator/index.js";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";

	const sidebar = Sidebar.useSidebar();

	interface Props {
		main?: string;
		sub?: string[];
	}
	let props: Props = $props();
</script>

<header
	class="bg-background sticky top-0 z-50 flex w-full items-center border-b uter"
	style="background-color: var(--dark);color:var(--light)"
>
	<div
		class="h-(--header-height) flex w-full items-center gap-2 px-4"
		style="background-color: var(--dark);color:var(--light)"
	>
		<Button class="size-8" variant="ghost" size="icon" onclick={sidebar.toggle}>
			<SidebarIcon />
		</Button>
		<Separator orientation="vertical" class="mr-2 h-4" />
		<Breadcrumb.Root class="hidden sm:block">
			<Breadcrumb.List>
				<Breadcrumb.Item
				style="background-color: var(--dark);color:var(--light)">Dashboard</Breadcrumb.Item>
				{#each props.sub ?? [] as segment}
					<Breadcrumb.Separator 
					style="background-color: var(--dark);color:var(--light)"/>
					<Breadcrumb.Item
					style="background-color: var(--dark);color:var(--light)">
						{segment}
					</Breadcrumb.Item>
				{/each}
			</Breadcrumb.List>
		</Breadcrumb.Root>
		<!-- <SearchForm class="w-full sm:ml-auto sm:w-auto" /> -->
	</div>
</header>
<style>
	.uter * {
		font-weight: 600;
	}
</style>