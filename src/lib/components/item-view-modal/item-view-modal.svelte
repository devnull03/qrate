<script lang="ts">
	import * as Dialog from "$lib/components/ui/dialog/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as kv from "idb-keyval";
	import { getUploadedFiles } from "$lib/utils/files";

	interface ItemViewModalProps {
		isOpen: boolean;
		img?: string;
		itemFields: Record<string, any> | null;
		filename: string;
		editingApi?: any;
	}

	let {
		isOpen = $bindable(false),
		img,
		itemFields,
		filename,
		editingApi
	}: ItemViewModalProps = $props();

	$inspect(itemFields);
	let m = $state(false);

	let questions = $state(
		(await kv.get("ai-results"))[filename].questions?.map((x) => ({
			label: x,
			value: ""
		})) ?? []
	);
	async function blobToBase64(blob: Blob): Promise<string> {
		return new Promise<string>((resolve, reject) => {
			const reader = new FileReader();
			reader.onload = () => {
				const result = reader.result as string;
				resolve(result.replace(/^data:image\/[a-z]+;base64,/, ""));
			};
			reader.onerror = (err) => reject(err);
			reader.readAsDataURL(blob);
		});
	}
	const send = async () => {
		const files = await getUploadedFiles();
		const f = await files?.find((x) => x[0] === filename)![1].getFile();
		m = true;
		const res = await (
			await fetch("/api/image", {
				method: "post",
				body: JSON.stringify({
					image: await blobToBase64(f as File),
					qna: questions.map((x: { label: any; value: any }) => [x.label, x.value])
				})
			})
		).json();
		questions = [];
		m = false;
		await kv.set("ai-results", { ...(await kv.get("ai-results")), [filename]: res });
		itemFields = { ...itemFields, ...res.response.metadata };
		res.response.metadata;
	};
</script>

<Dialog.Root bind:open={isOpen}>
	<Dialog.Content class="max-w-none! w-[80vw] h-[90vh] p-0 flex">
		<!-- Left side - Image and Fields (70%) -->
		<div class="flex-[0.7] flex flex-col p-6 border-r overflow-y-auto">
			<!-- Image section -->
			<div class="flex-shrink-0 mb-6">
				{#if img}
					<img
						src={img}
						alt={itemFields?.title ?? "Preview"}
						class="max-w-full w-full max-h-[40vh] object-contain rounded-lg shadow-sm"
					/>
				{/if}
			</div>

			<!-- Fields section -->
			<div class="flex-1">
				<h3 class="text-lg font-semibold mb-4">Item Details</h3>
				<div class="grid gap-3">
					{#each Object.entries(itemFields ?? {}) as [key, value]}
						<div class="flex flex-col gap-1">
							<span class="text-sm font-medium text-muted-foreground">{key}</span>
							<span class="text-sm bg-muted rounded-md p-2">{value}</span>
						</div>
					{/each}
				</div>
			</div>
		</div>

		<div class="flex-[0.3] flex flex-col">
			{#if questions.length >= 1 && !m}
				<Dialog.Header class="p-6 pb-4 border-b">
					<Dialog.Title>Questions</Dialog.Title>
					<Dialog.Description>Please answer the following questions:</Dialog.Description>
				</Dialog.Header>

				<!-- Chat messages area -->
				<div class="flex-1 overflow-y-auto p-2 space-y-2">
					{#each questions as q}
						<!-- AI Question -->
						<div class="flex gap-1">
							<div class="flex-1">
								<div class="text-sm">
									{q.label}
								</div>
							</div>
						</div>

						<!-- User Response -->
						<div class="flex gap-1 justify-end">
							<div class="flex-1 bg-muted">
								<input bind:value={q.value} style="width:100%;padding:.3em;" />
							</div>
						</div>
					{/each}
				</div>

				<!-- Chat input area -->
				<div class="p-6 pt-4 border-t">
					<div class="flex gap-2">
						<Button size="sm" onclick={send}>Send</Button>
					</div>
				</div>
			{:else}
				<div style="padding:3em">{m ? "Loading..." : "No questions."}</div>
			{/if}
		</div>
	</Dialog.Content>
</Dialog.Root>
