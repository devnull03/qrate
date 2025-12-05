export type AnnotationSeverity = 'info' | 'warning' | 'error';

export interface AnnotationReference {
	rowId: number | null;
	columnId: string | null;
}

export interface Annotation {
	id: number;
	provider: string;
	rowId: number | null;
	columnId: string | null;
	severity: AnnotationSeverity;
	message: string;
	createdAt: string;
	updatedAt: string;
	resolved: boolean;
	metadata?: Record<string, unknown>;
}

export interface CreateAnnotationInput {
	provider: string;
	rowId?: number | null;
	columnId?: string | null;
	severity?: AnnotationSeverity;
	message: string;
	metadata?: Record<string, unknown>;
}

export interface UpdateAnnotationInput {
	message?: string;
	resolved?: boolean;
	metadata?: Record<string, unknown>;
}

export interface IAnnotationAccessor {
	update(message: string): Promise<void>;
	resolve(): Promise<void>;
	unresolve(): Promise<void>;
	delete(): Promise<void>;
	getAnnotation(): Annotation | undefined;
}

export interface IAnnotationsService {
	readonly annotations: Annotation[];
	byProvider(provider: string): Annotation[];
	byReference(rowId: number | null, columnId: string | null): Annotation[];
	unresolvedCount(provider?: string): number;
	add(input: CreateAnnotationInput): Promise<IAnnotationAccessor>;
	get(id: number): Annotation | undefined;
	load(path: string, provider?: string): Promise<void>;
	clear(): void;
}
