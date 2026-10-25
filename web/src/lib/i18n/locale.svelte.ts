/** The UI language. Kept apart from the messages so `format.ts` can use it without cycles. */

export type Lang = 'en' | 'fi';
export const LANGS: { id: Lang; label: string }[] = [
	{ id: 'en', label: 'English' },
	{ id: 'fi', label: 'Suomi' }
];

const KEY = 'katsanto.lang';

export function isLang(v: unknown): v is Lang {
	return v === 'en' || v === 'fi';
}

function initial(): Lang {
	try {
		const saved = localStorage.getItem(KEY);
		if (isLang(saved)) return saved;
		return navigator.language.toLowerCase().startsWith('fi') ? 'fi' : 'en';
	} catch {
		return 'en';
	}
}

class Locale {
	current = $state<Lang>(initial());

	set(lang: Lang) {
		this.current = lang;
		try {
			document.documentElement.lang = lang;
			localStorage.setItem(KEY, lang);
		} catch {
			// Not in a browser, or storage is blocked.
		}
	}
}

export const locale = new Locale();
