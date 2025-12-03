<script lang="ts">
	import type {
		IStatusBarEntry,
		MarkdownString,
		TooltipWithActions,
	} from "$lib/models/statusbar";
	import { commandRegistry } from "$lib/services/statusbar";
	import { parseIconTokens } from "$lib/utils/iconToken";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";

	interface Props {
		entry: IStatusBarEntry;
		compact?: boolean;
	}

	let { entry, compact = false }: Props = $props();

	let parsedText = $derived(parseIconTokens(entry.text));
	let isClickable = $derived(!!entry.command);

	let tooltipText = $derived.by(() => {
		if (!entry.tooltip) return undefined;
		if (typeof entry.tooltip === "string") return entry.tooltip;
		if ("value" in entry.tooltip)
			return (entry.tooltip as MarkdownString).value;
		if ("text" in entry.tooltip) {
			const t = entry.tooltip as TooltipWithActions;
			return typeof t.text === "string" ? t.text : t.text.value;
		}
		return undefined;
	});

	let ariaLabel = $derived(
		entry.accessibilityLabel ||
			entry.name ||
			parsedText.plainText.trim() ||
			entry.id,
	);

	let role = $derived(entry.role || (isClickable ? "button" : "status"));

	let progressType = $derived.by(() => {
		if (!entry.showProgress) return null;
		if (entry.showProgress === "syncing") return "syncing";
		if (entry.showProgress === "loading" || entry.showProgress === true)
			return "loading";
		return null;
	});

	let kindClasses = $derived.by(() => {
		switch (entry.kind) {
			case "warning":
				return "text-yellow-500";
			case "error":
				return "text-destructive";
			case "prominent":
				return "bg-primary text-primary-foreground hover:bg-primary/90";
			default:
				return "";
		}
	});

	let inlineStyles = $derived.by(() => {
		const styles: string[] = [];
		if (entry.color) styles.push(`color: ${entry.color}`);
		if (entry.backgroundColor)
			styles.push(`background-color: ${entry.backgroundColor}`);
		return styles.length > 0 ? styles.join("; ") : undefined;
	});

	async function handleActivate() {
		if (!entry.command) return;
		const cmd = entry.command;
		const commandId = typeof cmd === "string" ? cmd : cmd.id;
		const args = typeof cmd === "string" ? [] : cmd.args || [];
		try {
			await commandRegistry.execute(commandId, ...args);
		} catch (error) {
			console.error(
				`[StatusBarItem] Failed to execute command "${commandId}":`,
				error,
			);
		}
	}

	function handleClick(event: MouseEvent) {
		if (isClickable) {
			event.preventDefault();
			handleActivate();
		}
	}
</script>

{#snippet itemContent()}
	{#if progressType}
		<span
			class="inline-flex items-center justify-center"
			aria-hidden="true"
		>
			{#if progressType === "syncing"}
				<RefreshCwIcon class="size-3 animate-spin" />
			{:else}
				<LoaderCircleIcon class="size-3 animate-spin" />
			{/if}
		</span>
	{/if}

	{#if entry.text}
		<span class="inline-flex items-center gap-0.5">
			{#each parsedText.segments as segment}
				{#if segment.type === "text"}
					<span class:hidden={compact}>{segment.value}</span>
				{:else}
					<span
						class="inline-flex items-center justify-center text-[0.625rem] opacity-50"
						class:animate-spin={segment.token.modifier === "spin"}
						data-icon={segment.token.name}
						aria-hidden="true"
					>
						[{segment.token.name}]
					</span>
				{/if}
			{/each}
		</span>
	{/if}

	{#if entry.showBeak}
		<span
			class="absolute bottom-full left-1/2 -translate-x-1/2 border-x-4 border-b-4 border-x-transparent border-b-primary"
			aria-hidden="true"
		></span>
	{/if}
{/snippet}

{#if isClickable}
	<button
		type="button"
		class="inline-flex h-full cursor-pointer items-center gap-1 whitespace-nowrap rounded-sm border-none bg-transparent px-1.5 font-inherit text-inherit transition-colors select-none hover:bg-muted-foreground/15 focus-visible:outline-1 focus-visible:outline-ring focus-visible:-outline-offset-1 {kindClasses}"
		class:relative={entry.showBeak}
		class:px-1={compact}
		aria-label={ariaLabel}
		title={tooltipText}
		style={inlineStyles}
		onclick={handleClick}
	>
		{@render itemContent()}
	</button>
{:else}
	<span
		class="inline-flex h-full items-center gap-1 whitespace-nowrap rounded-sm px-1.5 transition-colors select-none {kindClasses}"
		class:relative={entry.showBeak}
		class:px-1={compact}
		{role}
		aria-label={ariaLabel}
		aria-live={entry.ariaLive}
		title={tooltipText}
		style={inlineStyles}
	>
		{@render itemContent()}
	</span>
{/if}
