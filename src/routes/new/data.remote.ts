import { queryCohere } from '$lib/server/server';
import { query } from '$app/server';

export const queryCohereRemote = query("unchecked", async({qna,image}:{image: string, qna?: [q: string, a: string][]})=> {
    return queryCohere(image, qna)
})