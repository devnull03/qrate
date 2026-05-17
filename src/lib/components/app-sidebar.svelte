<script lang="ts" module>
	import SquareTerminalIcon from "@lucide/svelte/icons/square-terminal";
	import BotIcon from "@lucide/svelte/icons/bot";
	import BookOpenIcon from "@lucide/svelte/icons/book-open";
	import LifeBuoyIcon from "@lucide/svelte/icons/life-buoy";
	import SendIcon from "@lucide/svelte/icons/send";
	import FrameIcon from "@lucide/svelte/icons/frame";
	import CommandIcon from "@lucide/svelte/icons/command";

	const data = {
		user: {
			name: "shadcn",
			email: "m@example.com",
			avatar: ""
		},
		navMain: [
			{
				title: "Assist",
				url: resolve("/dashboard/assist"),
				icon: SquareTerminalIcon
			},
			{
				title: "Review",
				url: resolve("/dashboard/review"),
				icon: BotIcon
			},
			{
				title: "Docs",
				url: resolve("/dashboard/docs"),
				icon: BookOpenIcon
			},
			{
				title: "Visualize",
				url: resolve("/dashboard/visualize"),
				icon: ChartPie
			}	

		],

		navSecondary: [
			{
				title: "Support",
				url: "#",
				icon: LifeBuoyIcon
			},
			{
				title: "Feedback",
				url: "#",
				icon: SendIcon
			}
		],

		projects: [
			{
				name: "Default Local",
				url: "#",
				icon: FrameIcon
			},

		]
	};
</script>

<script lang="ts">
	import type { ComponentProps } from "svelte";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";
	import NavMain from "./nav-main.svelte";
	import NavProjects from "./nav-projects.svelte";
	import NavSecondary from "./nav-secondary.svelte";
	import NavUser from "./nav-user.svelte";
	import { resolve } from "$app/paths";
	import ChartPie from "@lucide/svelte/icons/pie-chart";

	let { ref = $bindable(null), ...restProps }: ComponentProps<typeof Sidebar.Root> = $props();
</script>

<Sidebar.Root class="top-(--header-height) h-[calc(100svh-var(--header-height))]!" {...restProps}>
	<Sidebar.Header>
		<Sidebar.Menu>
			<Sidebar.MenuItem>
				<Sidebar.MenuButton size="lg">
					{#snippet child({ props })}
						<a href="##" {...props}>
							<div
								class="bg-sidebar-primary text-sidebar-primary-foreground flex aspect-square size-8 items-center justify-center rounded-lg"
							>
								<CommandIcon class="size-4" />
							</div>
							<div class="grid flex-1 text-left text-sm leading-tight">
								<span class="truncate font-medium">qrate</span>
								<span class="truncate text-xs">Enterprise</span>
							</div>
						</a>
					{/snippet}
				</Sidebar.MenuButton>
			</Sidebar.MenuItem>
		</Sidebar.Menu>
	</Sidebar.Header>
	<Sidebar.Content>
		<NavMain items={data.navMain} />
		<NavProjects projects={data.projects} />
		<NavSecondary items={data.navSecondary} class="mt-auto" />
	</Sidebar.Content>
	<!-- <Sidebar.Footer>
		<NavUser user={data.user} />
	</Sidebar.Footer> -->
</Sidebar.Root>
