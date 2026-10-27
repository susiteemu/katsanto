import { session } from '#lib/session.svelte.js';

/** Where subtitles sit along the bottom of the video. */
export type SubtitlePosition = 'left' | 'center' | 'right';
export const POSITIONS: SubtitlePosition[] = ['left', 'center', 'right'];

export interface SubtitleSettings {
	position: SubtitlePosition;
	/** Language codes listed first in the captions menu, in this order. */
	preferred: string[];
	/** Hide languages written in a script other than Latin (preferred ones stay). */
	latinOnly: boolean;
}

/** The user's choices, completed with defaults. */
export function subtitleSettings(): SubtitleSettings {
	const raw = ((session.me?.settings ?? {}) as Record<string, unknown>).subtitles ?? {};
	const s = raw as Record<string, unknown>;
	return {
		position: POSITIONS.includes(s.position as SubtitlePosition)
			? (s.position as SubtitlePosition)
			: 'center',
		preferred: Array.isArray(s.preferred)
			? s.preferred.filter((l): l is string => typeof l === 'string')
			: [],
		latinOnly: s.latinOnly === true
	};
}

export function saveSubtitleSettings(s: SubtitleSettings) {
	session.saveSettings({ subtitles: s });
}

/** `pt-BR` → `pt`: YouTube's codes carry regions and scripts the settings don't. */
const base = (lang: string) => lang.split('-')[0].toLowerCase();

/** The writing system a language is usually written in (`Latn`, `Cyrl`, `Jpan`…), if known. */
export function scriptOf(lang: string): string | undefined {
	try {
		return new Intl.Locale(lang).maximize().script;
	} catch {
		return undefined;
	}
}

/** Caption tracks with preferred languages first and, if asked, non-Latin ones left out. */
export function arrangeCaptions<T extends { lang: string }>(tracks: T[], s: SubtitleSettings): T[] {
	const rank = (t: T) => {
		const i = s.preferred.indexOf(base(t.lang));
		return i < 0 ? s.preferred.length : i;
	};
	return tracks
		.filter((t) => {
			if (!s.latinOnly || rank(t) < s.preferred.length) return true;
			const script = scriptOf(t.lang);
			return !script || script === 'Latn';
		})
		.map((t, i) => ({ t, i, r: rank(t) }))
		.sort((a, b) => a.r - b.r || a.i - b.i)
		.map(({ t }) => t);
}

/**
 * Cue settings for a position. YouTube's VTT pins cues to the left edge
 * (`align:start position:0%`); this overrides that, leaving a small margin.
 */
export function placeCue(cue: VTTCue, position: SubtitlePosition) {
	if (position === 'left') {
		cue.align = 'start';
		cue.position = 3;
	} else if (position === 'right') {
		cue.align = 'end';
		cue.position = 97;
	} else {
		cue.align = 'center';
		cue.position = 'auto';
	}
	cue.positionAlign = 'auto';
	cue.size = 94;
}

/** ISO 639-1 languages, offered as preferred subtitle languages. */
export const LANGUAGE_CODES = (
	'aa ab af ak am an ar as av ay az ba be bg bi bm bn bo br bs ca ce ch co cr cs cu cv cy da de dv dz ' +
	'ee el en eo es et eu fa ff fi fj fo fr fy ga gd gl gn gu gv ha he hi ho hr ht hu hy hz ia id ie ig ' +
	'ii ik io is it iu ja jv ka kg ki kj kk kl km kn ko kr ks ku kv kw ky la lb lg li ln lo lt lu lv mg ' +
	'mh mi mk ml mn mr ms mt my na nb nd ne ng nl nn no nr nv ny oc oj om or os pa pi pl ps pt qu rm rn ' +
	'ro ru rw sa sc sd se sg si sk sl sm sn so sq sr ss st su sv sw ta te tg th ti tk tl tn to tr ts tt ' +
	'tw ty ug uk ur uz ve vi vo wa wo xh yi yo za zh zu'
).split(' ');
