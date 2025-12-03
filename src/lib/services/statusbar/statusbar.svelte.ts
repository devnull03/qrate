import type {
	IStatusBarEntry,
	IStatusBarAccessor,
	IStatusBarEntryInternal,
} from "$lib/models/statusbar";

function compareEntries(
	a: IStatusBarEntryInternal,
	b: IStatusBarEntryInternal,
): number {
	const priorityA = a.priority ?? 0;
	const priorityB = b.priority ?? 0;

	if (priorityA !== priorityB) return priorityB - priorityA;
	return a._insertionOrder - b._insertionOrder;
}

function createStatusBarService() {
	let entriesMap = $state<Map<string, IStatusBarEntryInternal>>(new Map());
	let insertionCounter = 0;

	const leftEntries = $derived.by(() =>
		Array.from(entriesMap.values())
			.filter(
				(e) =>
					(e.alignment ?? "left") === "left" && e.visible !== false,
			)
			.sort(compareEntries),
	);

	const rightEntries = $derived.by(() =>
		Array.from(entriesMap.values())
			.filter((e) => e.alignment === "right" && e.visible !== false)
			.sort(compareEntries),
	);

	function createAccessor(id: string): IStatusBarAccessor {
		return {
			update: (updates) => updateEntry(id, updates),
			show: () => updateEntry(id, { visible: true }),
			hide: () => updateEntry(id, { visible: false }),
			dispose: () => removeEntry(id),
			getEntry: () => entriesMap.get(id),
		};
	}

	function updateEntry(
		id: string,
		updates: Partial<Omit<IStatusBarEntry, "id">>,
	): void {
		const existing = entriesMap.get(id);
		if (!existing) return;

		const newMap = new Map(entriesMap);
		newMap.set(id, { ...existing, ...updates });
		entriesMap = newMap;
	}

	function addEntry(entry: IStatusBarEntry): IStatusBarAccessor {
		const internalEntry: IStatusBarEntryInternal = {
			...entry,
			visible: entry.visible ?? true,
			_insertionOrder: insertionCounter++,
		};

		const newMap = new Map(entriesMap);
		newMap.set(entry.id, internalEntry);
		entriesMap = newMap;

		return createAccessor(entry.id);
	}

	function setEntry(
		id: string,
		entry: Omit<IStatusBarEntry, "id">,
	): IStatusBarAccessor {
		const existing = entriesMap.get(id);
		const insertionOrder = existing?._insertionOrder ?? insertionCounter++;

		const internalEntry: IStatusBarEntryInternal = {
			...entry,
			id,
			visible: entry.visible ?? true,
			_insertionOrder: insertionOrder,
		};

		const newMap = new Map(entriesMap);
		newMap.set(id, internalEntry);
		entriesMap = newMap;

		return createAccessor(id);
	}

	function removeEntry(id: string): void {
		if (entriesMap.has(id)) {
			const newMap = new Map(entriesMap);
			newMap.delete(id);
			entriesMap = newMap;
		}
	}

	function getEntry(id: string): IStatusBarEntry | undefined {
		return entriesMap.get(id);
	}

	function hasEntry(id: string): boolean {
		return entriesMap.has(id);
	}

	function clear(): void {
		entriesMap = new Map();
		insertionCounter = 0;
	}

	return {
		get leftEntries() {
			return leftEntries;
		},
		get rightEntries() {
			return rightEntries;
		},
		get entries(): ReadonlyMap<string, IStatusBarEntry> {
			return entriesMap;
		},
		addEntry,
		setEntry,
		removeEntry,
		getEntry,
		hasEntry,
		clear,
		subscribe(
			callback: (entries: ReadonlyMap<string, IStatusBarEntry>) => void,
		): () => void {
			callback(entriesMap);
			return () => {};
		},
	};
}

export const statusBarService = createStatusBarService();
export { createStatusBarService };
