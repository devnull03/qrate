import type { PageData, PageLoad } from './$types';

type DisplayMode = 'spreadsheet' | 'grid';


export const load: PageLoad = ({ url }): PageData => {
	const todoParam = url.searchParams.get('t');
	const doneParam = url.searchParams.get('d');

	const mapParamToDisplayMode = (param: string | null): DisplayMode => {
		switch (param) {
			case 's':
				return 'spreadsheet';
			case 'g':
				return 'grid';
			default:
				return "grid";
		}
	};

	return {
		todoDisplayMode: mapParamToDisplayMode(todoParam),
		doneDisplayMode: mapParamToDisplayMode(doneParam)
	};
};
