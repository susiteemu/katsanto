/** `364` → `6:04`, `3723` → `1:02:03`. */
export function duration(s?: number | null): string {
	if (s == null || !Number.isFinite(s)) return '';
	const t = Math.max(0, Math.floor(s));
	const h = Math.floor(t / 3600);
	const m = Math.floor(t / 60) % 60;
	const sec = String(t % 60).padStart(2, '0');
	return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}

import { m } from './i18n/index.js';
import { locale, type Lang } from './i18n/locale.svelte.js';

/** Intl formatters per language, created on first use. */
function cached<T>(make: (lang: Lang) => T): () => T {
	const made = new Map<Lang, T>();
	return () => {
		const lang = locale.current;
		let f = made.get(lang);
		if (!f) made.set(lang, (f = make(lang)));
		return f;
	};
}

const compactFmt = cached(
	(lang) => new Intl.NumberFormat(lang, { notation: 'compact', maximumFractionDigits: 1 })
);
const numberFmt = cached((lang) => new Intl.NumberFormat(lang));
const rtf = cached((lang) => new Intl.RelativeTimeFormat(lang, { numeric: 'auto' }));
const dateFmt = cached((lang) => new Intl.DateTimeFormat(lang, { dateStyle: 'medium' }));

/** `1234567` → `1.2M`. */
export function compact(n?: number | null): string {
	return n == null ? '' : compactFmt().format(n);
}

/** `1234567` → `1,234,567`. */
export function number(n?: number | null): string {
	return n == null ? '' : numberFmt().format(n);
}

/** Unix seconds → `Oct 24, 2026`. */
export function date(unix?: number | null): string {
	return unix == null ? '' : dateFmt().format(unix * 1000);
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
	['year', 365 * 86400],
	['month', 30 * 86400],
	['week', 7 * 86400],
	['day', 86400],
	['hour', 3600],
	['minute', 60]
];

/** Unix seconds → "3 days ago". */
export function ago(unix?: number | null, now = Date.now() / 1000): string {
	if (unix == null) return '';
	const diff = unix - now;
	for (const [unit, secs] of UNITS) {
		if (Math.abs(diff) >= secs) return rtf().format(Math.round(diff / secs), unit);
	}
	return m.common.justNow;
}

/** Watched fraction 0–1, or null when there's nothing to show. */
export function watchedFraction(
	p?: { position_s: number; duration_s: number; finished: boolean } | null
) {
	if (!p || p.duration_s <= 0) return null;
	return p.finished ? 1 : Math.min(1, p.position_s / p.duration_s);
}
