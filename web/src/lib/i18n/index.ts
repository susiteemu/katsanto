import { en, type Messages } from './en.js';
import { fi } from './fi.js';
import { locale } from './locale.svelte.js';

export { LANGS, isLang, locale, type Lang } from './locale.svelte.js';
export type { Messages };

const DICTS = { en, fi } satisfies Record<string, Messages>;

/**
 * The UI messages in the current language: `m.nav.home`, `m.lists.videos(3)`.
 * Reading through this proxy tracks the language, so templates update when it changes.
 */
export const m: Messages = new Proxy({} as Messages, {
	get: (_, key) => DICTS[locale.current][key as keyof Messages]
});

/** A list's name; the built-in Watch later list is shown in the UI language unless renamed. */
export function listName(l: { kind: string; name: string }): string {
	return l.kind === 'watch_later' && l.name === 'Watch later' ? m.lists.watchLater : l.name;
}
