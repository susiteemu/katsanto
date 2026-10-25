import { api, unwrap, type Schemas } from '#lib/api/client.js';
import { session } from '#lib/session.svelte.js';

/** What happens when playback reaches a SponsorBlock segment. */
export type SegmentAction = 'skip' | 'button' | 'mark' | 'off';

export const CATEGORIES = [
	'sponsor',
	'selfpromo',
	'interaction',
	'intro',
	'outro',
	'preview',
	'hook',
	'music_offtopic',
	'filler',
	'poi_highlight'
] as const;
export type Category = (typeof CATEGORIES)[number];

export interface CommunitySettings {
	sponsorblock: { enabled: boolean; categories: Record<Category, SegmentAction> };
	dearrow: { titles: boolean; thumbnails: boolean };
}

/** Like the SponsorBlock extension: sponsors are skipped, the rest is offered or marked. */
export const DEFAULT_ACTIONS: Record<Category, SegmentAction> = {
	sponsor: 'skip',
	selfpromo: 'button',
	interaction: 'button',
	intro: 'button',
	outro: 'button',
	preview: 'mark',
	hook: 'mark',
	music_offtopic: 'off',
	filler: 'off',
	poi_highlight: 'button'
};

/** Timeline colors, as used by SponsorBlock. */
export const CATEGORY_COLORS: Record<Category, string> = {
	sponsor: '#00d400',
	selfpromo: '#ffff00',
	interaction: '#cc00ff',
	intro: '#00ffff',
	outro: '#0202ed',
	preview: '#008fd6',
	hook: '#395699',
	music_offtopic: '#ff9900',
	filler: '#7300ff',
	poi_highlight: '#ff1684'
};

const ACTIONS: SegmentAction[] = ['skip', 'button', 'mark', 'off'];

/** The user's choices, completed with defaults. */
export function communitySettings(): CommunitySettings {
	const raw = (session.me?.settings ?? {}) as Record<string, unknown>;
	const sb = (raw.sponsorblock ?? {}) as Record<string, unknown>;
	const cats = (sb.categories ?? {}) as Record<string, unknown>;
	const da = (raw.dearrow ?? {}) as Record<string, unknown>;
	const categories = { ...DEFAULT_ACTIONS };
	for (const c of CATEGORIES) {
		if (ACTIONS.includes(cats[c] as SegmentAction)) categories[c] = cats[c] as SegmentAction;
	}
	return {
		sponsorblock: { enabled: sb.enabled !== false, categories },
		dearrow: { titles: da.titles === true, thumbnails: da.thumbnails === true }
	};
}

export function saveCommunitySettings(s: CommunitySettings) {
	session.saveSettings({ sponsorblock: s.sponsorblock, dearrow: s.dearrow });
}

/** Integrations this instance has switched on (loaded once). */
class Features {
	value = $state<Schemas['Features'] | null>(null);
	#loading: Promise<void> | null = null;

	load() {
		this.#loading ??= unwrap(api.GET('/api/v1/features')).then(
			(f) => void (this.value = f),
			() => void (this.#loading = null)
		);
		return this.#loading;
	}
}

export const features = new Features();
