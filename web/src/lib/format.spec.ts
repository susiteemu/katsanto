import { describe, expect, it } from 'vitest';
import { ago, compact, duration, watchedFraction } from './format.js';
import { locale } from './i18n/index.js';

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

	it('watched fraction', () => {
		expect(watchedFraction({ position_s: 30, duration_s: 120, finished: false })).toBe(0.25);
		expect(watchedFraction({ position_s: 10, duration_s: 120, finished: true })).toBe(1);
		expect(watchedFraction(null)).toBeNull();
	});
});
