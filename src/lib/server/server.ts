import { COHERE_API_KEY } from "$env/static/private";
import type { RequestHandler } from "@sveltejs/kit";
import { CohereClientV2 } from "cohere-ai";

const prompt = `
You are an archivist assistant trained to follow the South Asian Canadian Digital Archive (SACDA) Metadata Manual.  
Your goal is to extract complete and accurate metadata for archival images.  

1. Analyze the image and generate metadata and descriptions.
2. If any fields are missing or uncertain, or any important details are unknown or unclear (such as identities of people for the description), output clarifying questions to the user.
 - Examples: "What is the identity of the person on the left?"
 - Include more questions about the description of the image to get all relevant details.  
3. When the user provides answers, incorporate them into the metadata and update the output. is_done MUST be true after the answers to clarifying questions are given.
4. If is_done is true, then all fields should be filled.

Rules:
- If any field cannot be determined, leave it as an empty string "" (not null).
- Add a clear and concise follow-up question for each missing or uncertain field.
- Use **Rules for Archival Description (RAD)** when applicable.
- Dates must follow **ISO 8601 (YYYY-MM-DD)**.
- Geographic locations: use **CGNDB** (Canada) or **TGN** (outside Canada).
- Coordinates: always in **decimal degrees**.
- Subjects: use **LCSH**, **TGM**, or **SACDA Thesaurus** (narrowest possible heading).
- Rights: select exactly one value from **rightsstatement.org** list in the SACDA manual.
- Continue asking questions until all fields are filled or explicitly confirmed as “unknown”.
- When all fields are completed or confirmed, set \`"is_done": true\` and return \`"questions": []\`.
- Do Not bring up ANY of the raw field names in the clarifying questions. ALL QUESTIoNS MUST BE IN NATURAL LANGUAGE
- Do not waste critical and vital archivist time by asking useless questions
- Do not create potential for gaps in vital information by neglecting to ask for vital info
- Don't just ask a question for every field. Be creative! TRY To do as much on your own as possible  

The assistant must never break JSON format and must always include all fields in \`"metadata"\`.

Example: (populate ...)
\`\`\`json
{
  "is_done": boolean,          // true if all fields are filled or confirmed as unknown
  "metadata": {                    // all metadata fields must always be present
    "fileTitle": "International Mother Language Day, 2004 ", // just an example do not use
    "title": "[Photograph of a woman addressing the audience at the Punjabi Language Education Association's Language Day event]", // just an example do not use
    "field_linked_agent": "Chandra Bodalia", // just an example do not use
    "field_extent": "1 photograph : col. negative", // just an example do not use
    "field_description": "...", // at least three paragraphs
    "field_rights": "CC BY-NC-SA 4.0", // just an example do not use
    "field_resource_type": "still image", // just an example do not use
    "field_language": "...",
    "field_note": "...",
    "field_subject": "Events; Language education; Speeches, addresses, etc., Canadian; Panjabi language", // just an example do not use
    "field_subjects_name": "...",
    "field_subject_name__organization": "Punjabi Language Education Association", // just an example do not use
    "field_geographic_subject": "surrey_bc", // just an example do not use
    "field_coordinates": "49.111667, -122.8275", // just an example do not use
  },
  "questions": [
    "string"
  ]
}
\`\`\`
`;

const responseFormat = {
	type: "json_object" as const,
	properties: {
		is_done: {
			type: "boolean",
			description:
				"True if all metadata fields are confidently filled, false if any field is missing or requires clarification"
		},
		data: {
			type: "object",
			properties: {
				/*accessIdentifier: {
					type: "string",
					description: "Unique identifier used to access the resource"
				},*/
				//parent_id: { type: "string", description: "Identifier of the parent record or object" },
				fileTitle: { type: "string", description: "Title of the file" },
				title: {
					type: "string",
					description:
						'General title of the record or resource (example: "[Photograph of a woman addressing the audience at the Punjabi Language Education Association\'s Language Day event]")'
				},
				field_linked_agent: {
					type: "string",
					description: "Agent (person, organization) linked to this resource (depicted)"
				},
				//field_edtf_date_created: {
				//	type: "string",
				//	description: "Date the resource was created, in EDTF format"
				//},
				field_extent: {
					type: "string",
					description: 'Extent or size of the resource (ex "1 photograph : col. negative")'
				},
				field_description: {
					type: "string",
					description: "Description of the resource, multiple paragraphs (3) if possible"
				},
				field_rights: {
					type: "string",
					description: 'Rights or usage restrictions for the resource (ex "CC BY-NC-SA 4.0")'
				},
				field_resource_type: {
					type: "string",
					description: 'Type of the resource (example: "still image")'
				},
				field_language: { type: "string", description: "Language(s) of the resource" },
				field_note: { type: "string", description: "Additional notes related to the resource" },
				field_subject: {
					type: "string",
					description:
						'Subject or topic of the resource (example: "Events; Language education; Speeches, addresses, etc., Canadian; Panjabi language")'
				},
				//field_sacda_thesaurus: {
				//	type: "string",
				//	description: "Controlled vocabulary or thesaurus term (SACDA)"
				//}, ToDo
				field_subjects_name: { type: "string", description: "Name of the subject entity" },
				field_subject_name__organization: {
					type: "string",
					description: "Organization associated with the subject"
				},
				field_geographic_subject: {
					type: "string",
					description:
						'Geographic subject or location associated with the resource (ex "surrey_bc")'
				},
				field_coordinates: {
					type: "string",
					description: "Geographic coordinates related to the resource"
				}
				//field_member_of: {
				//	type: "string",
				//	description: "Collection or group this resource belongs to"
				//},
				//file: { type: "string", description: "File path or identifier" },
				//file_extention: { type: "string", description: "File extension (e.g., pdf, jpg, txt)" },
				//field_viewer_override: {
				//	type: "string",
				//	description: "Override option for viewer or display settings"
				//}
			},
			required: [
				//"accessIdentifier",
				//"parent_id",
				"fileTitle",
				"title",
				"field_linked_agent",
				//"field_edtf_date_created",
				"field_extent",
				"field_description",
				"field_rights",
				"field_resource_type",
				"field_language",
				"field_note",
				"field_subject",
				//"field_sacda_thesaurus",
				"field_subjects_name",
				"field_subject_name__organization",
				"field_geographic_subject",
				"field_coordinates"
				//"field_member_of",
				//"file",
				//"file_extention",
				//"field_viewer_override"
			]
		},
		questions: {
			type: "array",
			items: { type: "string" },
			description: "List of follow-up questions for the user to clarify missing or uncertain fields"
		}
	},
	required: ["is_done", "data", "questions"]
};

export const cohere = new CohereClientV2({
	token: COHERE_API_KEY
});

export async function queryCohere(image: string, qna?: [q: string, a: string][]) {
	const response = await cohere.chat({
		model: "command-a-vision-07-2025",
		messages: [
			{
				role: "system",
				content: [{ type: "text", text: prompt }]
			},
			{
				role: "user",
				content: [{ type: "image_url", imageUrl: { url: `data:image/png;base64,${image}` } } as any]
			},
			...(qna
				? [
						{
							role: "user" as const,
							content: [
								{
									type: "text" as const,
									text: `Here are the answers to the clarifying questions:\n${qna.map(
										(x) => `Q: "${x[0]}", A: ${JSON.stringify(x[1])}`
									)}`
								}
							]
						}
					]
				: [])
		],
		responseFormat: responseFormat
	});

	return JSON.parse((response.message.content as any)[0].text);
}
