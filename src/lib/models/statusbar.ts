import type { Component } from "svelte";

export interface MarkdownString {
	value: string;
	isTrusted?: boolean;
}

export interface Command {
	id: string;
	title?: string;
	args?: unknown[];
}

export interface TooltipWithActions {
	text: string | MarkdownString;
	actions?: Command[];
}

export type StatusBarEntryKind = "standard" | "warning" | "error" | "prominent";
export type ProgressType = boolean | "loading" | "syncing";
export type StatusBarAlignment = "left" | "right";

export interface IStatusBarEntry {
	id: string;
	name?: string;
	text?: string;
	tooltip?: string | MarkdownString | TooltipWithActions;
	command?: string | Command;
	alignment?: StatusBarAlignment;
	priority?: number;
	showProgress?: ProgressType;
	color?: string;
	backgroundColor?: string;
	content?: HTMLElement | (() => HTMLElement) | Component;
	accessibilityLabel?: string;
	role?: "button" | "status" | string;
	visible?: boolean;
	kind?: StatusBarEntryKind;
	showBeak?: boolean;
	ariaLive?: "off" | "polite" | "assertive";
}

export interface IStatusBarAccessor {
	update(updates: Partial<Omit<IStatusBarEntry, "id">>): void;
	show(): void;
	hide(): void;
	dispose(): void;
	getEntry(): IStatusBarEntry | undefined;
}

export interface IStatusBarService {
	addEntry(entry: IStatusBarEntry): IStatusBarAccessor;
	setEntry(id: string, entry: Omit<IStatusBarEntry, "id">): IStatusBarAccessor;
	removeEntry(id: string): void;
	getEntry(id: string): IStatusBarEntry | undefined;
	hasEntry(id: string): boolean;
	readonly entries: ReadonlyMap<string, IStatusBarEntry>;
	subscribe(callback: (entries: ReadonlyMap<string, IStatusBarEntry>) => void): () => void;
}

export interface IStatusBarEntryInternal extends IStatusBarEntry {
	_insertionOrder: number;
}
