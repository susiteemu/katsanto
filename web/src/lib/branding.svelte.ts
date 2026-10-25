import { SvelteMap } from 'svelte/reactivity';
import { api, unwrap, type Schemas } from '#lib/api/client.js';
import { communitySettings } from '#lib/community.svelte.js';

type Branding = Schemas['VideoBranding'];

/** Ids per request; the server's limit. */
const BATCH = 60;

/**
 * DeArrow titles and thumbnails. Cards ask for their video as they render; the ids
 * are collected for a moment and looked up in batches. `null`: nothing submitted.
 */
class BrandingStore {
	#known = new SvelteMap<string, Branding | null>();
	#queue = new Set<string>();
	#timer: ReturnType<typeof setTimeout> | undefined;

	/** The branding for a video, or undefined while unknown (a lookup is queued). */
	get(id: string): Branding | null | undefined {
		if (!this.#known.has(id) && !this.#queue.has(id)) {
			this.#queue.add(id);
			this.#timer ??= setTimeout(() => this.#flush(), 50);
		}
		return this.#known.get(id);
	}

	async #flush() {
		this.#timer = undefined;
		const ids = [...this.#queue];
		this.#queue.clear();
		for (let i = 0; i < ids.length; i += BATCH) {
			const batch = ids.slice(i, i + BATCH);
			try {
				const found = await unwrap(api.POST('/api/v1/branding', { body: { ids: batch } }));
				for (const id of batch) this.#known.set(id, found[id] ?? null);
			} catch {
				// Leave them unknown: YouTube's title and thumbnail stay.
			}
		}
	}
}

const store = new BrandingStore();

/** Title and thumbnail to show for a video, with DeArrow applied if the user wants it. */
export function branded(video: { id: string; title: string; thumbnail_url?: string | null }) {
	const { titles, thumbnails } = communitySettings().dearrow;
	const b = titles || thumbnails ? store.get(video.id) : undefined;
	const title = (titles && b?.title) || video.title;
	return {
		title,
		thumbnail_url: (thumbnails && b?.thumbnail_url) || video.thumbnail_url || null,
		/** YouTube's title, when a different one is shown. */
		original: title !== video.title ? video.title : null
	};
}
