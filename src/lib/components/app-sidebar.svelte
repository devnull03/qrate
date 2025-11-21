<script lang="ts" module>
	import InboxIcon from "@lucide/svelte/icons/inbox";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import FileSpreadsheetIcon from "@lucide/svelte/icons/file-spreadsheet";

	const data = {
		user: {
			name: "User",
			email: "user@example.com",
			avatar: "",
		},
		navMain: [
			{
				title: "Inbox",
				url: "#",
				icon: InboxIcon,
				isActive: true,
			},
			{
				title: "Load CSV",
				url: "#",
				icon: FolderOpenIcon,
				isActive: false,
			},
			{
				title: "Data View",
				url: "#",
				icon: FileSpreadsheetIcon,
				isActive: false,
			},
		],
	};
</script>

<script lang="ts">
	import NavUser from "./nav-user.svelte";
	import { Label } from "$lib/components/ui/label/index.js";
	import { useSidebar } from "$lib/components/ui/sidebar/context.svelte.js";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";
	import { Switch } from "$lib/components/ui/switch/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import CommandIcon from "@lucide/svelte/icons/command";
	import type { ComponentProps } from "svelte";
	import { open } from "@tauri-apps/plugin-dialog";
	import { csvStore } from "$lib/stores/csvStore.svelte";

	let {
		ref = $bindable(null),
		...restProps
	}: ComponentProps<typeof Sidebar.Root> = $props();

	let activeItem = $state(data.navMain[0]);
	const sidebar = useSidebar();

	async function handleLoadCsv() {
		try {
			const result = await open({
				multiple: false,
				filters: [
					{
						name: "CSV",
						extensions: ["csv"],
					},
				],
			});

			if (result) {
				await csvStore.loadCsv(result);
				sidebar.setOpen(true);
			}
		} catch (error) {
			console.error("Failed to open file:", error);
		}
	}
</script>

<Sidebar.Root
	bind:ref
	collapsible="icon"
	class="overflow-hidden [&>[data-sidebar=sidebar]]:flex-row"
	{...restProps}
>
	<!-- This is the first sidebar -->
	<!-- We disable collapsible and adjust width to icon. -->
	<!-- This will make the sidebar appear as icons. -->
	<Sidebar.Root
		collapsible="none"
		class="!w-[calc(var(--sidebar-width-icon)_+_1px)] border-r"
	>
		<Sidebar.Header>
			<Sidebar.Menu>
				<Sidebar.MenuItem>
					<Sidebar.MenuButton
						size="lg"
						class="md:h-8 md:p-0"
					>
						{#snippet child({ props })}
							<a href="##" {...props}>
								<div
									class="bg-sidebar-primary text-sidebar-primary-foreground flex aspect-square size-8 items-center justify-center rounded-lg"
								>
									<CommandIcon
										class="size-4"
									/>
								</div>
								<div
									class="grid flex-1 text-left text-sm leading-tight"
								>
									<span
										class="truncate font-medium"
										>Qrate</span
									>
									<span
										class="truncate text-xs"
										>CSV
										Viewer</span
									>
								</div>
							</a>
						{/snippet}
					</Sidebar.MenuButton>
				</Sidebar.MenuItem>
			</Sidebar.Menu>
		</Sidebar.Header>
		<Sidebar.Content>
			<Sidebar.Group>
				<Sidebar.GroupContent class="px-1.5 md:px-0">
					<Sidebar.Menu>
						{#each data.navMain as item (item.title)}
							<Sidebar.MenuItem>
								<Sidebar.MenuButton
									tooltipContentProps={{
										hidden: false,
									}}
									onclick={() => {
										if (
											item.title ===
											"Load CSV"
										) {
											handleLoadCsv();
										}
										activeItem =
											item;
										sidebar.setOpen(
											true,
										);
									}}
									isActive={activeItem.title ===
										item.title}
									class="px-2.5 md:px-2"
								>
									{#snippet tooltipContent()}
										{item.title}
									{/snippet}
									<item.icon
									/>
									<span
										>{item.title}</span
									>
								</Sidebar.MenuButton>
							</Sidebar.MenuItem>
						{/each}
					</Sidebar.Menu>
				</Sidebar.GroupContent>
			</Sidebar.Group>
		</Sidebar.Content>
		<Sidebar.Footer>
			<NavUser user={data.user} />
		</Sidebar.Footer>
	</Sidebar.Root>

	<!-- This is the second sidebar -->
	<!-- We disable collapsible and let it fill remaining space -->
	<Sidebar.Root collapsible="none" class="hidden flex-1 md:flex">
		<Sidebar.Header class="gap-3.5 border-b p-4">
			<div class="flex w-full items-center justify-between">
				<div
					class="text-foreground text-base font-medium"
				>
					{activeItem.title}
				</div>
				{#if activeItem.title === "Load CSV"}
					<Button
						onclick={handleLoadCsv}
						size="sm"
						variant="outline"
					>
						Open CSV File
					</Button>
				{/if}
			</div>
			{#if csvStore.data.length > 0}
				<div class="text-sm text-muted-foreground">
					Loaded {csvStore.data.length} rows
				</div>
			{/if}
		</Sidebar.Header>
		<Sidebar.Content>
			<Sidebar.Group class="px-0">
				<Sidebar.GroupContent>
					{#if csvStore.isLoading}
						<div
							class="flex items-center justify-center p-8"
						>
							<div
								class="text-sm text-muted-foreground"
							>
								Loading CSV...
							</div>
						</div>
					{:else if csvStore.error}
						<div
							class="flex flex-col items-center justify-center p-8 gap-2"
						>
							<div
								class="text-sm text-destructive"
							>
								{csvStore.error}
							</div>
							<Button
								onclick={handleLoadCsv}
								size="sm"
								variant="outline"
							>
								Try Again
							</Button>
						</div>
					{:else if csvStore.data.length > 0}
						<div class="p-4 space-y-2">
							<div
								class="text-xs font-medium text-muted-foreground mb-3"
							>
								CSV Preview
								(First 10 rows)
							</div>
							{#each csvStore.data.slice(0, 10) as row, idx}
								<div
									class="hover:bg-sidebar-accent hover:text-sidebar-accent-foreground flex flex-col gap-1 whitespace-nowrap border-b p-3 text-sm leading-tight last:border-b-0 rounded-sm"
								>
									<div
										class="flex items-center gap-2"
									>
										<span
											class="text-xs text-muted-foreground"
											>Row
											{idx +
												1}</span
										>
									</div>
									<div
										class="flex flex-col gap-1"
									>
										{#each Object.entries(row) as [key, value]}
											<div
												class="flex gap-2"
											>
												<span
													class="text-xs font-medium text-muted-foreground min-w-[80px]"
													>{key}:</span
												>
												<span
													class="text-xs truncate"
													>{value}</span
												>
											</div>
										{/each}
									</div>
								</div>
							{/each}
						</div>
					{:else}
						<div
							class="flex flex-col items-center justify-center p-8 gap-4"
						>
							<FileSpreadsheetIcon
								class="size-12 text-muted-foreground"
							/>
							<div
								class="text-sm text-muted-foreground text-center"
							>
								No CSV file
								loaded.<br />
								Click "Load CSV"
								to get started.
							</div>
							<Button
								onclick={handleLoadCsv}
								size="sm"
								variant="outline"
							>
								Open CSV File
							</Button>
						</div>
					{/if}
				</Sidebar.GroupContent>
			</Sidebar.Group>
		</Sidebar.Content>
	</Sidebar.Root>
</Sidebar.Root>
