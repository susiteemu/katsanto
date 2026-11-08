import { describe, expect, it } from 'vitest';
import { ago, compact, duration, streamTime, watchedFraction, when } from './format.js';
import { locale } from './i18n/index.js';

/** Plain spaces, whichever ones ICU uses. */
const sp = (s: string | null) => s?.replace(/\s/g, ' ') ?? null;

describe('format', () => {
	it('durations', () => {
		expect(duration(364)).toBe('6:04');
		expect(duration(3723)).toBe('1:02:03');
		expect(duration(59.9)).toBe('0:59');
		expect(duration(null)).toBe('');
	});

	it('compact numbers', () => {
		expect(compact(1_234_567)).toBe('1.2M');
		expect(compact(999)).toBe('999');
		expect(compact(undefined)).toBe('');
	});

	it('relative dates', () => {
		const now = 1_000_000_000;
		expect(ago(now - 3 * 86400, now)).toBe('3 days ago');
		expect(ago(now - 30, now)).toBe('just now');
		expect(ago(now - 400 * 86400, now)).toBe('last year');
	});

	it('follows the UI language', () => {
		const now = 1_000_000_000;
		locale.set('fi');
		try {
			expect(ago(now - 3 * 86400, now)).toBe('3 päivää sitten');
			expect(ago(now - 30, now)).toBe('juuri nyt');
			expect(compact(1_234_567)).toBe('1,2\u00a0milj.');
		} finally {
			locale.set('en');
		}
	});

	it('times near now, in local time', () => {
		// Some ICU versions put a narrow no-break space before "PM".
		const at = (d: number, h: number, min = 0) => new Date(2026, 10, d, h, min).getTime() / 1000;
		const now = at(8, 9); // Sunday
		expect(sp(when(at(8, 17), now))).toBe('today 5:00 PM');
		expect(sp(when(at(8, 17), now, false))).toBe('5:00 PM');
		expect(sp(when(at(9, 17), now))).toBe('tomorrow 5:00 PM');
		expect(sp(when(at(7, 17), now))).toBe('yesterday 5:00 PM');
		expect(sp(when(at(13, 17), now))).toBe('Fri 5:00 PM');
		expect(sp(when(at(20, 17, 30), now))).toBe('Nov 20, 5:30 PM');
	});

	it('stream times', () => {
		const now = new Date(2026, 10, 8, 9).getTime() / 1000;
		const stream = (is_upcoming: boolean, is_live: boolean, start: number, end?: number) => ({
			is_upcoming,
			is_live,
			live_at: now + start,
			live_ended_at: end == null ? null : now + end
		});
		expect(sp(streamTime(stream(true, true, 25 * 60), now))).toBe('Starts in 25 minutes');
		expect(sp(streamTime(stream(true, true, 8 * 3600), now))).toBe('Starts today 5:00 PM');
		expect(sp(streamTime(stream(true, true, -120), now))).toBe('Starting soon');
		expect(sp(streamTime(stream(false, true, -3600), now))).toBe('Live since 8:00 AM');
		expect(sp(streamTime(stream(false, false, -7 * 3600, -6 * 3600), now))).toBe(
			'Streamed 7 hours ago'
		);
		expect(
			streamTime({ is_upcoming: false, is_live: false, live_at: null, live_ended_at: null }, now)
		).toBe(null);
	});

	it('watched fraction', () => {
		expect(watchedFraction({ position_s: 30, duration_s: 120, finished: false })).toBe(0.25);
		expect(watchedFraction({ position_s: 10, duration_s: 120, finished: true })).toBe(1);
		expect(watchedFraction(null)).toBeNull();
	});
});
