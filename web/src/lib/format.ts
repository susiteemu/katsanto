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
const timeFmt = cached(
	(lang) => new Intl.DateTimeFormat(lang, { hour: 'numeric', minute: '2-digit' })
);
const weekdayFmt = cached(
	(lang) => new Intl.DateTimeFormat(lang, { weekday: 'short', hour: 'numeric', minute: '2-digit' })
);
const dayFmt = cached(
	(lang) =>
		new Intl.DateTimeFormat(lang, {
			month: 'short',
			day: 'numeric',
			hour: 'numeric',
			minute: '2-digit'
		})
);

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

const midnight = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();

/**
 * Unix seconds → a local time near now: "today 5:00 PM" (or just "5:00 PM" without
 * `sayToday`), "tomorrow 5:00 PM", "Sat 5:00 PM" within the next week, otherwise
 * "Nov 12, 5:00 PM".
 */
export function when(unix: number, now = Date.now() / 1000, sayToday = true): string {
	const d = new Date(unix * 1000);
	const days = Math.round((midnight(d) - midnight(new Date(now * 1000))) / 86_400_000);
	const time = timeFmt().format(d);
	if (days === 0 && !sayToday) return time;
	if (Math.abs(days) <= 1) return `${rtf().format(days, 'day')} ${time}`;
	if (days > 1 && days < 7) return weekdayFmt().format(d);
	return dayFmt().format(d);
}

/** What a live stream's card says about its time; null for other videos. */
export function streamTime(
	v: {
		is_live: boolean;
		is_upcoming: boolean;
		live_at?: number | null;
		live_ended_at?: number | null;
	},
	now = Date.now() / 1000
): string | null {
	if (v.live_at == null) return null;
	if (v.is_upcoming) {
		const left = v.live_at - now;
		if (left <= 60) return m.video.startingSoon;
		return m.video.starts(left < 3600 ? ago(v.live_at, now) : when(v.live_at, now));
	}
	if (v.is_live) return m.video.liveSince(when(v.live_at, now, false));
	if (v.live_ended_at != null) return m.video.streamed(ago(v.live_at, now));
	return null;
}

/** Watched fraction 0–1, or null when there's nothing to show. */
export function watchedFraction(
	p?: { position_s: number; duration_s: number; finished: boolean } | null
) {
	if (!p || p.duration_s <= 0) return null;
	return p.finished ? 1 : Math.min(1, p.position_s / p.duration_s);
}
