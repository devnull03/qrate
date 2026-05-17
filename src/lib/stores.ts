import { writable } from "svelte/store";

export const curMode = writable<"Create" | "Review">("Create");
