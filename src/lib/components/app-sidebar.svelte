<script lang="ts" module>
	import InboxIcon from "@lucide/svelte/icons/inbox";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import FileSpreadsheetIcon from "@lucide/svelte/icons/file-spreadsheet";
	import FilePlusIcon from "@lucide/svelte/icons/file-plus";
	import UploadIcon from "@lucide/svelte/icons/upload";

	const data = {
		user: {
			name: "User",
			email: "user@example.com",
			avatar: "",
		},
		navMain: [
			{
				title: "File",
				url: "#",
				icon: InboxIcon,
				isActive: true,
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
	import { useSidebar } from "$lib/components/ui/sidebar/context.svelte.js";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import CommandIcon from "@lucide/svelte/icons/command";
	import type { ComponentProps } from "svelte";
	import { open, save } from "@tauri-apps/plugin-dialog";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";

	let {
		ref = $bindable(null),
		...restProps
	}: ComponentProps<typeof Sidebar.Root> = $props();

	let activeItem = $state(data.navMain[0]);
	let isProcessing = $state(false);
	const sidebar = useSidebar();

	// Current file info
	let currentFileName = $derived.by(() => {
		if (!qrateStore.currentFilePath) return null;
		const parts = qrateStore.currentFilePath.split(/[\\/]/);
		return parts[parts.length - 1];
	});

	let fileStats = $derived.by(() => {
		if (!qrateStore.isFileOpen) return null;
		return {
			rows: qrateStore.totalRows,
			columns: qrateStore.columns.length,
		};
	});

	/**
	 * Open an existing .qrate file
	 */
	async function handleOpenQrate() {
		try {
			isProcessing = true;

			const selected = await open({
				multiple: false,
				filters: [
					{
						name: "Qrate Files",
						extensions: ["qrate"],
					},
				],
			});

			if (selected && typeof selected === "string") {
				await qrateStore.openFile(selected);
			}
		} catch (err) {
			console.error("Failed to open .qrate file:", err);
		} finally {
			isProcessing = false;
		}
	}

	/**
	 * Create a new .qrate file
	 */
	async function handleCreateQrate() {
		try {
			isProcessing = true;

			const selected = await save({
				filters: [
					{
						name: "Qrate Files",
						extensions: ["qrate"],
					},
				],
				defaultPath: "untitled.qrate",
			});

			if (selected) {
				await qrateStore.createFile(selected);
			}
		} catch (err) {
			console.error("Failed to create .qrate file:", err);
		} finally {
			isProcessing = false;
		}
	}

	/**
	 * Import a CSV file into a new or existing .qrate file
	 */
	async function handleImportCsv() {
		try {
			isProcessing = true;

			// First, select the CSV file to import
			const csvFile = await open({
				multiple: false,
				filters: [
					{
						name: "CSV Files",
						extensions: ["csv"],
					},
				],
			});

			if (!csvFile || typeof csvFile !== "string") {
				isProcessing = false;
				return;
			}

			// Then, select where to save the .qrate file
			const qrateFile = await save({
				filters: [
					{
						name: "Qrate Files",
						extensions: ["qrate"],
					},
				],
				defaultPath: csvFile.replace(
					/\.csv$/i,
					".qrate",
				),
			});

			if (!qrateFile) {
				isProcessing = false;
				return;
			}

			// Import the CSV data
			await qrateStore.importCsv(qrateFile, csvFile);
		} catch (err) {
			console.error("Failed to import CSV:", err);
		} finally {
			isProcessing = false;
		}
	}

	/**
	 * Close the current file
	 */
	async function handleCloseFile() {
		try {
			await qrateStore.closeFile();
		} catch (err) {
			console.error("Failed to close file:", err);
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
										>qRate</span
									>
									<span
										class="truncate text-xs"
										>Archival
										Workspace</span
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
			</div>
			{#if qrateStore.isFileOpen && currentFileName && fileStats}
				<div class="p-3 bg-muted rounded-md space-y-1">
					<p
						class="text-sm font-medium truncate"
						title={qrateStore.currentFilePath}
					>
						{currentFileName}
					</p>
					<p
						class="text-xs text-muted-foreground"
					>
						{fileStats.rows.toLocaleString()}
						rows × {fileStats.columns} columns
					</p>
				</div>
			{/if}
		</Sidebar.Header>
		<Sidebar.Content>
			<Sidebar.Group class="px-0">
				<Sidebar.GroupContent>
					{#if activeItem.title === "File"}
						<div class="p-4 space-y-4">
							{#if !qrateStore.isFileOpen}
								<div
									class="space-y-2"
								>
									<Button
										onclick={handleOpenQrate}
										disabled={isProcessing}
										class="w-full justify-start"
										variant="outline"
									>
										<FolderOpenIcon
											class="mr-2 size-4"
										/>
										Open
										.qrate
										File
									</Button>

									<Button
										onclick={handleCreateQrate}
										disabled={isProcessing}
										class="w-full justify-start"
										variant="outline"
									>
										<FilePlusIcon
											class="mr-2 size-4"
										/>
										New
										.qrate
										File
									</Button>

									<div
										class="relative"
									>
										<div
											class="absolute inset-0 flex items-center"
										>
											<span
												class="w-full border-t border-border"

											></span>
										</div>
										<div
											class="relative flex justify-center text-xs uppercase"
										>
											<span
												class="bg-sidebar px-2 text-muted-foreground"
												>Or</span
											>
										</div>
									</div>

									<Button
										onclick={handleImportCsv}
										disabled={isProcessing}
										class="w-full justify-start"
									>
										<UploadIcon
											class="mr-2 size-4"
										/>
										Import
										CSV
									</Button>
								</div>
							{:else}
								<Button
									onclick={handleCloseFile}
									disabled={isProcessing}
									class="w-full justify-start"
									variant="outline"
								>
									Close
									File
								</Button>
							{/if}

							{#if isProcessing}
								<div
									class="text-sm text-muted-foreground text-center"
								>
									Processing...
								</div>
							{/if}

							<div
								class="pt-4 border-t border-border space-y-2 text-xs text-muted-foreground"
							>
								<h3
									class="font-semibold text-foreground"
								>
									About
									qRate
								</h3>
								<p>
									Digital
									archival
									workspace
									for
									managing
									cultural
									heritage
									metadata.
								</p>
								<ul
									class="list-disc list-inside space-y-1 ml-2"
								>
									<li>
										Instant
										loading
										of
										massive
										datasets
									</li>
									<li>
										Crash-proof
										ACID
										transactions
									</li>
									<li>
										Standards-ready
										(RAD,
										MODS)
									</li>
									<li>
										AI-assisted
										cataloging
									</li>
								</ul>
							</div>
						</div>
					{:else if activeItem.title === "Data View"}
						<div class="p-4">
							{#if qrateStore.isFileOpen}
								<div
									class="space-y-2 text-sm"
								>
									<div
										class="flex justify-between"
									>
										<span
											class="text-muted-foreground"
											>Total
											Rows:</span
										>
										<span
											class="font-medium"
											>{qrateStore.totalRows.toLocaleString()}</span
										>
									</div>
									<div
										class="flex justify-between"
									>
										<span
											class="text-muted-foreground"
											>Columns:</span
										>
										<span
											class="font-medium"
											>{qrateStore
												.columns
												.length}</span
										>
									</div>
									<div
										class="flex justify-between"
									>
										<span
											class="text-muted-foreground"
											>Loaded
											Rows:</span
										>
										<span
											class="font-medium"
											>{qrateStore
												.rows
												.length}</span
										>
									</div>
								</div>
							{:else}
								<div
									class="text-sm text-muted-foreground text-center"
								>
									No file
									open
								</div>
							{/if}
						</div>
					{/if}
				</Sidebar.GroupContent>
			</Sidebar.Group>
		</Sidebar.Content>
	</Sidebar.Root>
</Sidebar.Root>
