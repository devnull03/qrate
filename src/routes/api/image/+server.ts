import { cohere, queryCohere } from "$lib/server/server";
import { json, type RequestHandler } from "@sveltejs/kit";

export const POST: RequestHandler = async ({ request, cookies }) => {
	const { image, qna } = await request.json();

	const response = await queryCohere(image, qna);

	return json({ response }, { status: 200 });
};