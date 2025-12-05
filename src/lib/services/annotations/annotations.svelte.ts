import { invoke } from "@tauri-apps/api/core";
import type {
	Annotation,
	CreateAnnotationInput,
	UpdateAnnotationInput,
	IAnnotationAccessor,
} from "$lib/models/annotations";

interface BackendAnnotation {
	id: number;
	provider: string;
	row_id: number | null;
	column_id: string | null;
	severity: string;
	message: string;
	created_at: string;
	updated_at: string;
	resolved: boolean;
	metadata?: Record<string, unknown>;
}

function toFrontendAnnotation(backend: BackendAnnotation): Annotation {
	return {
		id: backend.id,
		provider: backend.provider,
		rowId: backend.row_id,
		columnId: backend.column_id,
		severity: backend.severity as Annotation["severity"],
		message: backend.message,
		createdAt: backend.created_at,
		updatedAt: backend.updated_at,
		resolved: backend.resolved,
		metadata: backend.metadata,
	};
}

function createAnnotationsService() {
	let annotationsMap = $state<Map<number, Annotation>>(new Map());
	let currentPath = $state<string | null>(null);

	const annotations = $derived.by(() =>
		Array.from(annotationsMap.values()).sort(
			(a, b) =>
				new Date(b.createdAt).getTime() -
				new Date(a.createdAt).getTime(),
		),
	);

	const unresolvedCount = $derived.by(
		() => Array.from(annotationsMap.values()).filter((a) => !a.resolved).length,
	);

	function byProvider(provider: string): Annotation[] {
		return annotations.filter((a) => a.provider === provider);
	}

	function byReference(
		rowId: number | null,
		columnId: string | null,
	): Annotation[] {
		return annotations.filter(
			(a) => a.rowId === rowId && a.columnId === columnId,
		);
	}

	function getUnresolvedCount(provider?: string): number {
		const filtered = provider
			? annotations.filter((a) => a.provider === provider)
			: annotations;
		return filtered.filter((a) => !a.resolved).length;
	}

	function get(id: number): Annotation | undefined {
		return annotationsMap.get(id);
	}

	function createAccessor(id: number): IAnnotationAccessor {
		return {
			update: async (message: string) => {
				await updateAnnotation(id, { message });
			},
			resolve: async () => {
				await updateAnnotation(id, { resolved: true });
			},
			unresolve: async () => {
				await updateAnnotation(id, { resolved: false });
			},
			delete: async () => {
				await deleteAnnotation(id);
			},
			getAnnotation: () => annotationsMap.get(id),
		};
	}

	async function load(path: string, provider?: string): Promise<void> {
		currentPath = path;

		const result = await invoke<BackendAnnotation[]>("get_annotations", {
			path,
			provider: provider ?? null,
		});

		const newMap = new Map<number, Annotation>();
		for (const backend of result) {
			newMap.set(backend.id, toFrontendAnnotation(backend));
		}
		annotationsMap = newMap;
	}

	async function add(input: CreateAnnotationInput): Promise<IAnnotationAccessor> {
		if (!currentPath) {
			throw new Error("No file is currently open");
		}

		const result = await invoke<BackendAnnotation>("create_annotation", {
			path: currentPath,
			annotation: {
				provider: input.provider,
				rowId: input.rowId ?? null,
				columnId: input.columnId ?? null,
				severity: input.severity ?? "info",
				message: input.message,
				metadata: input.metadata ?? null,
			},
		});

		const annotation = toFrontendAnnotation(result);
		const newMap = new Map(annotationsMap);
		newMap.set(annotation.id, annotation);
		annotationsMap = newMap;

		return createAccessor(annotation.id);
	}

	async function updateAnnotation(
		id: number,
		updates: UpdateAnnotationInput,
	): Promise<void> {
		if (!currentPath) {
			throw new Error("No file is currently open");
		}

		const result = await invoke<BackendAnnotation>("update_annotation", {
			path: currentPath,
			id,
			updates,
		});

		const annotation = toFrontendAnnotation(result);
		const newMap = new Map(annotationsMap);
		newMap.set(annotation.id, annotation);
		annotationsMap = newMap;
	}

	async function deleteAnnotation(id: number): Promise<void> {
		if (!currentPath) {
			throw new Error("No file is currently open");
		}

		await invoke("delete_annotation", {
			path: currentPath,
			id,
		});

		const newMap = new Map(annotationsMap);
		newMap.delete(id);
		annotationsMap = newMap;
	}

	function clear(): void {
		annotationsMap = new Map();
		currentPath = null;
	}

	return {
		get annotations() {
			return annotations;
		},
		get unresolvedCount() {
			return unresolvedCount;
		},
		get currentPath() {
			return currentPath;
		},
		byProvider,
		byReference,
		getUnresolvedCount,
		get,
		load,
		add,
		clear,
	};
}

export const annotationsService = createAnnotationsService();
export { createAnnotationsService };
