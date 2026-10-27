import { m } from '#lib/i18n/index.js';

export const youtubeUrl = (id: string) => `https://www.youtube.com/watch?v=${id}`;

/** Copies the video's YouTube link; shows it in a prompt when the clipboard isn't available. */
export async function copyYouTubeLink(id: string): Promise<boolean> {
	const url = youtubeUrl(id);
	try {
		await navigator.clipboard.writeText(url);
		return true;
	} catch {
		// No clipboard outside a secure context (plain http).
		prompt(m.share.copyPrompt, url);
		return false;
	}
}

/**
 * Opens the system share sheet for the video's YouTube link, or copies the link where
 * there is none. Resolves to true when the link was copied.
 */
export async function shareYouTubeLink(id: string, title: string): Promise<boolean> {
	if (navigator.share) {
		try {
			await navigator.share({ title, url: youtubeUrl(id) });
			return false;
		} catch (e) {
			if (e instanceof DOMException && e.name === 'AbortError') return false;
			// Not allowed here; copy instead.
		}
	}
	return copyYouTubeLink(id);
}
