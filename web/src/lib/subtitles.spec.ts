import { describe, expect, it } from 'vitest';
import { arrangeCaptions, scriptOf, type SubtitleSettings } from './subtitles.svelte.js';

const tracks = ['ar', 'en', 'ja', 'fi', 'pt-BR', 'zh-Hans', 'sv', 'ru'].map((lang) => ({ lang }));
const langs = (s: Partial<SubtitleSettings>) =>
	arrangeCaptions(tracks, { position: 'center', preferred: [], latinOnly: false, ...s }).map(
		(t) => t.lang
	);

describe('arrangeCaptions', () => {
	it('keeps the order without settings', () => {
		expect(langs({})).toEqual(tracks.map((t) => t.lang));
	});

	it('puts preferred languages first, in preference order, matching regions', () => {
		expect(langs({ preferred: ['sv', 'pt', 'fi'] })).toEqual([
			'sv',
			'pt-BR',
			'fi',
			'ar',
			'en',
			'ja',
			'zh-Hans',
			'ru'
		]);
	});

	it('hides non-Latin scripts but keeps preferred ones', () => {
		expect(langs({ latinOnly: true, preferred: ['ja'] })).toEqual([
			'ja',
			'en',
			'fi',
			'pt-BR',
			'sv'
		]);
	});
});

describe('scriptOf', () => {
	it('knows common scripts', () => {
		expect(scriptOf('fi')).toBe('Latn');
		expect(scriptOf('ru')).toBe('Cyrl');
		expect(scriptOf('zh-Hans')).toBe('Hans');
		expect(scriptOf('not a tag')).toBeUndefined();
	});
});
