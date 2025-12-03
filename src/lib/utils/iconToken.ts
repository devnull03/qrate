/**
 * Icon Token Parser Utility
 *
 * Parses VS Code-style icon tokens from text strings.
 * Tokens follow the format: $(icon-name) or $(icon-name~modifier)
 *
 * Examples:
 * - "$(sync) Syncing" -> icon: "sync", text: " Syncing"
 * - "$(check~spin) Loading" -> icon: "check", modifier: "spin", text: " Loading"
 * - "$(folder) Files $(file)" -> multiple icons with text between
 */

/**
 * Parsed icon token result
 */
export interface IconToken {
	/** Icon name (without $() wrapper) */
	name: string;
	/** Optional modifier (e.g., "spin" for animation) */
	modifier?: string;
	/** Original token string including $() */
	raw: string;
}

/**
 * Segment of parsed text - either plain text or an icon token
 */
export type TextSegment =
	| { type: "text"; value: string }
	| { type: "icon"; token: IconToken };

/**
 * Result of parsing a text string with icon tokens
 */
export interface ParsedText {
	/** Array of text and icon segments in order */
	segments: TextSegment[];
	/** Whether any icon tokens were found */
	hasIcons: boolean;
	/** Plain text with icons removed */
	plainText: string;
}

// Regex to match $(icon-name) or $(icon-name~modifier)
// Icon names can contain: letters, numbers, hyphens, underscores
// Modifiers are separated by ~ and follow the same naming rules
const ICON_TOKEN_REGEX = /\$\(([a-zA-Z0-9_-]+)(?:~([a-zA-Z0-9_-]+))?\)/g;

/**
 * Parse a text string and extract icon tokens
 *
 * @param text - Text potentially containing icon tokens
 * @returns Parsed result with segments, hasIcons flag, and plain text
 *
 * @example
 * parseIconTokens("$(sync~spin) Syncing files...")
 * // Returns:
 * // {
 * //   segments: [
 * //     { type: "icon", token: { name: "sync", modifier: "spin", raw: "$(sync~spin)" } },
 * //     { type: "text", value: " Syncing files..." }
 * //   ],
 * //   hasIcons: true,
 * //   plainText: " Syncing files..."
 * // }
 */
export function parseIconTokens(text: string | undefined | null): ParsedText {
	if (!text) {
		return { segments: [], hasIcons: false, plainText: "" };
	}

	const segments: TextSegment[] = [];
	let lastIndex = 0;
	let hasIcons = false;
	let plainText = "";

	// Reset regex state
	ICON_TOKEN_REGEX.lastIndex = 0;

	let match: RegExpExecArray | null;
	while ((match = ICON_TOKEN_REGEX.exec(text)) !== null) {
		hasIcons = true;

		// Add text before this match
		if (match.index > lastIndex) {
			const textValue = text.slice(lastIndex, match.index);
			segments.push({ type: "text", value: textValue });
			plainText += textValue;
		}

		// Add the icon token
		const token: IconToken = {
			name: match[1],
			modifier: match[2],
			raw: match[0],
		};
		segments.push({ type: "icon", token });

		lastIndex = match.index + match[0].length;
	}

	// Add remaining text after last match
	if (lastIndex < text.length) {
		const textValue = text.slice(lastIndex);
		segments.push({ type: "text", value: textValue });
		plainText += textValue;
	}

	return { segments, hasIcons, plainText };
}

/**
 * Extract just the icon names from a text string
 *
 * @param text - Text potentially containing icon tokens
 * @returns Array of icon names found in the text
 *
 * @example
 * extractIconNames("$(folder) Files $(file)")
 * // Returns: ["folder", "file"]
 */
export function extractIconNames(text: string | undefined | null): string[] {
	if (!text) return [];

	const names: string[] = [];
	ICON_TOKEN_REGEX.lastIndex = 0;

	let match: RegExpExecArray | null;
	while ((match = ICON_TOKEN_REGEX.exec(text)) !== null) {
		names.push(match[1]);
	}

	return names;
}

/**
 * Remove all icon tokens from text, leaving only plain text
 *
 * @param text - Text potentially containing icon tokens
 * @returns Text with all icon tokens removed
 *
 * @example
 * stripIconTokens("$(sync) Syncing...")
 * // Returns: " Syncing..."
 */
export function stripIconTokens(text: string | undefined | null): string {
	if (!text) return "";
	return text.replace(ICON_TOKEN_REGEX, "");
}

/**
 * Check if a string contains any icon tokens
 *
 * @param text - Text to check
 * @returns True if the text contains at least one icon token
 */
export function hasIconTokens(text: string | undefined | null): boolean {
	if (!text) return false;
	ICON_TOKEN_REGEX.lastIndex = 0;
	return ICON_TOKEN_REGEX.test(text);
}

/**
 * Replace icon tokens with a custom replacement
 *
 * @param text - Text containing icon tokens
 * @param replacer - Function that receives the icon token and returns replacement
 * @returns Text with icon tokens replaced
 *
 * @example
 * replaceIconTokens("$(sync) Syncing", (token) => `[${token.name}]`)
 * // Returns: "[sync] Syncing"
 */
export function replaceIconTokens(
	text: string | undefined | null,
	replacer: (token: IconToken) => string
): string {
	if (!text) return "";

	return text.replace(ICON_TOKEN_REGEX, (raw, name, modifier) => {
		return replacer({ name, modifier, raw });
	});
}

/**
 * Common icon names used in status bars (for reference/autocomplete)
 */
export const COMMON_STATUS_ICONS = [
	// State icons
	"check",
	"x",
	"warning",
	"error",
	"info",
	"question",

	// Action icons
	"sync",
	"refresh",
	"loading",
	"play",
	"stop",
	"pause",

	// File icons
	"file",
	"folder",
	"folder-opened",
	"file-text",
	"file-code",

	// Editor icons
	"edit",
	"save",
	"trash",
	"copy",
	"clipboard",

	// Source control icons
	"git-branch",
	"git-commit",
	"git-pull-request",
	"git-merge",

	// Misc icons
	"bell",
	"gear",
	"search",
	"terminal",
	"debug",
	"extensions",
	"remote",
	"cloud",
	"cloud-upload",
	"cloud-download",
] as const;

export type CommonStatusIcon = (typeof COMMON_STATUS_ICONS)[number];
