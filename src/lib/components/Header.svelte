<script lang="ts">
	import * as Breadcrumb from "$lib/components/ui/breadcrumb/index.js";
	import { Separator } from "$lib/components/ui/separator/index.js";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";
	import ModeToggle from "$lib/components/ModeToggle.svelte";
</script>

<header
	class="bg-background sticky top-0 flex shrink-0 items-center gap-2 border-b p-4"
>
	<Sidebar.Trigger class="-ml-1" />
	<Separator
		orientation="vertical"
		class="mr-2 data-[orientation=vertical]:h-4"
	/>
	<Breadcrumb.Root>
		<Breadcrumb.List>
			<Breadcrumb.Item class="hidden md:block">
				<Breadcrumb.Link href="##">All Inboxes</Breadcrumb.Link>
			</Breadcrumb.Item>
			<Breadcrumb.Separator class="hidden md:block" />
			<Breadcrumb.Item>
				<Breadcrumb.Page>Inbox</Breadcrumb.Page>
			</Breadcrumb.Item>
		</Breadcrumb.List>
	</Breadcrumb.Root>

	<div class="ml-auto">
		<ModeToggle />
	</div>
</header>
