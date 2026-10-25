import { describe, expect, it } from 'vitest';
import { nextPath } from './nav.js';

const at = (next: string | null) =>
	nextPath(
		new URL(next === null ? 'http://x/login' : `http://x/login?next=${encodeURIComponent(next)}`)
	);

describe('nextPath', () => {
	it('keeps local paths', () => {
		expect(at('/search?q=rust')).toBe('/search?q=rust');
		expect(at('/settings')).toBe('/settings');
	});

	it('rejects other sites', () => {
		expect(at('https://evil.example')).toBe('/');
		expect(at('//evil.example')).toBe('/');
		expect(at('/\\evil.example')).toBe('/');
		expect(at('javascript:alert(1)')).toBe('/');
	});

	it('defaults to home', () => {
		expect(at(null)).toBe('/');
		expect(at('')).toBe('/');
	});
});
