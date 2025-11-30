<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import { Switch } from "$lib/components/ui/switch/index.js";
	import type { SettingMetadata } from "$lib/settings/schema";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import { open } from "@tauri-apps/plugin-dialog";

	interface Props {
		id: string;
		metadata: SettingMetadata;
		value: unknown;
		disabled?: boolean;
		onchange?: (value: unknown) => void;
	}

	let {
		id,
		metadata,
		value = $bindable(),
		disabled = false,
		onchange,
	}: Props = $props();

	// Handle value changes
	function handleChange(newValue: unknown) {
		value = newValue;
		onchange?.(newValue);
	}

	// Handle number input
	function handleNumberChange(e: Event) {
		const target = e.target as HTMLInputElement;
		const num = parseFloat(target.value);
		if (!isNaN(num)) {
			handleChange(num);
		}
	}

	// Handle string input
	function handleStringChange(e: Event) {
		const target = e.target as HTMLInputElement;
		handleChange(target.value);
	}

	// Handle boolean toggle
	function handleBooleanChange(checked: boolean) {
		handleChange(checked);
	}

	// Handle select option click
	function handleSelectOption(optionValue: string) {
		handleChange(optionValue);
	}

	// Handle path selection
	async function handlePathSelect() {
		try {
			const folder = await open({
				directory: true,
				multiple: false,
				title: `Select ${metadata.label}`,
			});

			if (folder && typeof folder === "string") {
				handleChange(folder);
			}
		} catch (err) {
			console.error("Failed to select folder:", err);
		}
	}
</script>

<div class="space-y-2">
	<Label for={id}>{metadata.label}</Label>

	{#if metadata.type === "boolean"}
		<div class="flex items-center gap-3">
			<Switch
				{id}
				checked={Boolean(value)}
				onCheckedChange={handleBooleanChange}
				{disabled}
			/>
			<span class="text-sm text-muted-foreground">
				{value ? "Enabled" : "Disabled"}
			</span>
		</div>
	{:else if metadata.type === "select"}
		<div class="flex flex-wrap gap-2">
			{#if metadata.options}
				{#each metadata.options as option}
					<Button
						variant={value === option.value ? "default" : "outline"}
						size="sm"
						class="flex-1"
						onclick={() => handleSelectOption(option.value)}
						{disabled}
					>
						{option.label}
					</Button>
				{/each}
			{/if}
		</div>
	{:else if metadata.type === "number"}
		<Input
			{id}
			type="number"
			value={String(value)}
			min={metadata.min}
			max={metadata.max}
			step={metadata.step}
			onchange={handleNumberChange}
			{disabled}
		/>
	{:else if metadata.type === "path"}
		<div class="flex gap-2">
			<Input
				{id}
				type="text"
				value={String(value || "")}
				placeholder={metadata.placeholder}
				readonly
				{disabled}
			/>
			<Button variant="outline" onclick={handlePathSelect} {disabled}>
				<FolderOpenIcon class="size-4" />
			</Button>
		</div>
	{:else if metadata.type === "pattern"}
		<Input
			{id}
			type="text"
			value={String(value || "")}
			placeholder={metadata.placeholder}
			onchange={handleStringChange}
			{disabled}
		/>
	{:else}
		<!-- string type (default) -->
		<Input
			{id}
			type="text"
			value={String(value || "")}
			placeholder={metadata.placeholder}
			onchange={handleStringChange}
			{disabled}
		/>
	{/if}

	{#if metadata.description}
		<p class="text-xs text-muted-foreground">
			{metadata.description}
		</p>
	{/if}
</div>
