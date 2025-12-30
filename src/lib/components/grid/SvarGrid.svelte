<script lang="ts">
	import { qrateStore, type ColumnDef } from "$lib/stores";
	import {
		ContextMenu,
		Grid,
		type IApi,
		type IColumnConfig,
	} from "@svar-ui/svelte-grid";
	import { WillowDark } from "@svar-ui/svelte-grid";
    import IndexCell from "./IndexCell.svelte";

	let gridApi: IApi | undefined = $state(undefined);

	const convertColumns = (columnData: ColumnDef[]): IColumnConfig[] => {
		const commonConfig = {
			resize: true,
			editor: "text",
			sort: true,
		};

		let editedColumnData = columnData.map((column) => {
			return {
				...column,
				...commonConfig,
				header: [
					column.name,
					{
						filter: {
							type: "text",
							config: {
								clear: true,
							},
						},
					},
				],
			};
		});

		// @ts-ignore
		editedColumnData.unshift({
			id: "id",
			width: 50,
			// @ts-ignore
			cell: IndexCell
		});

		// @ts-ignore
		return editedColumnData;
	};

	const columns = $derived(convertColumns(qrateStore.columns));

	// SVAR Grid needs $state for data to be reactive
	let data = $state<Record<string, any>[]>([]);

	// Sync data with store whenever rows change
	$effect(() => {
		data = [...qrateStore.rows];
	});
</script>

<div class="size-full h-[calc(100vh-6.1rem)]">
	<WillowDark>
		<ContextMenu bind:api={gridApi}>
			<Grid
				{data}
				{columns}
				reorder
				multiselect
				init={(api) => (gridApi = api)}
				split={{ left: 1 }}
			/>
		</ContextMenu>
	</WillowDark>
</div>
