<script lang="ts">
	import { getUploadedFiles } from "$lib/utils/files";
	import { createProjectWithAI, projectExists, clearProject } from "$lib/utils/project";
	import { goto } from "$app/navigation";
	import type { Image } from "$lib/interfaces/sheet.interface";

	let folder = $state<[string, FileSystemFileHandle][]>([]);
	let isCreating = $state(false);
	let progress = $state({ current: 0, total: 0, filename: "" });

	// Load existing folder on mount
	folder = (await getUploadedFiles()) ?? [];

	const selectFolder = async () => {
		try {
			isCreating = true;
			progress = { current: 0, total: 0, filename: "Selecting folder..." };

			const dir = await showDirectoryPicker();
			await dir.requestPermission();

			const entries: Image[] = [];
			for await (const entry of dir.entries()) {
				if (entry[1].kind !== "file") continue;
				entries.push(entry as Image);
			}

			if (entries.length === 0) {
				alert("No images found in the selected folder.");
				return;
			}

			progress = { current: 0, total: entries.length, filename: "Starting AI processing..." };

			// Clear existing project if any
			await clearProject();

			// Create project with AI metadata generation
			const project = await createProjectWithAI(
				entries,
				dir,
				{ name: "New Project" },
				(current, total, filename) => {
					progress = { current, total, filename: `Processing ${filename}...` };
				}
			);

			if (project) {
				progress = {
					current: progress.total,
					total: progress.total,
					filename: "Project created successfully!"
				};
				// Small delay to show success message
				setTimeout(() => {
					goto("./dashboard/assist");
				}, 1000);
			} else {
				throw new Error("Failed to create project");
			}
		} catch (error) {
			console.error("Error creating project:", error);
			alert("Failed to create project. Please try again.");
		} finally {
			isCreating = false;
		}
	};

	const openExistingProject = async () => {
		if (await projectExists()) {
			goto("./dashboard/assist");
		} else {
			alert("No existing project found. Please create a new project.");
		}
	};
</script>

<article>
	{#if isCreating}
		<div class="creating-project">
			<h2>Creating Project...</h2>
			<div class="progress-info">
				<p>{progress.filename}</p>
				{#if progress.total > 0}
					<div class="progress-bar">
						<div
							class="progress-fill"
							style="width: {(progress.current / progress.total) * 100}%"
						></div>
					</div>
					<p class="progress-text">{progress.current} / {progress.total} images processed</p>
				{/if}
			</div>
		</div>
	{:else}
		{#if folder.length}
			<button onclick={openExistingProject}>
				<h2>Existing Project</h2>
				<p>{folder.length} images</p>
			</button>
		{/if}
		<button onclick={selectFolder}>
			<h2>New Project</h2>
			<p>
				{folder.length
					? "WARNING: This will delete your current project."
					: "Creates a new project."}
			</p>
		</button>
	{/if}
</article>

<style>
	article {
		display: flex;
		gap: 1em;
		min-height: 200px;
		align-items: center;
		justify-content: center;
	}

	h2 {
		font-weight: bold;
	}

	button {
		background-color: var(--dark);
		color: var(--light);
		border: var(--dark) 2px solid;
		padding: 0.5em 1em;
		cursor: pointer;
		&:hover {
			background-color: var(--light);
			color: var(--dark);
		}
	}

	.creating-project {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 1.5rem;
		padding: 2rem;
		text-align: center;
	}

	.progress-info {
		display: flex;
		flex-direction: column;
		gap: 1rem;
		min-width: 300px;
	}

	.progress-bar {
		width: 100%;
		height: 8px;
		background-color: #e0e0e0;
		border-radius: 4px;
		overflow: hidden;
	}

	.progress-fill {
		height: 100%;
		background-color: var(--dark);
		transition: width 0.3s ease;
		border-radius: 4px;
	}

	.progress-text {
		font-size: 0.9rem;
		color: #666;
		margin: 0;
	}
</style>
