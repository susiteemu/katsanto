import { describe, expect, it } from 'vitest';
import { channelRuns } from './feedGroups.js';

const feed = (channels: string) =>
	[...channels].map((c) => ({ video: { channel: c === '-' ? null : { id: c } } }));
const runs = (channels: string) =>
	channelRuns(feed(channels)).map((r) => `${r.start}-${r.end}${r.stack ? ' stack' : ''}`);

describe('channelRuns', () => {
	it('handles an empty feed', () => {
		expect(runs('')).toEqual([]);
	});

	it('leaves runs of up to three videos alone', () => {
		expect(runs('aaabbbc')).toEqual(['0-7']);
	});

	it('stacks four or more consecutive videos', () => {
		expect(runs('aaaa')).toEqual(['0-4 stack']);
		expect(runs('baaaaac')).toEqual(['0-1', '1-6 stack', '6-7']);
	});

	it('keeps neighbouring stacks apart', () => {
		expect(runs('aaaabbbb')).toEqual(['0-4 stack', '4-8 stack']);
	});

	it('only counts consecutive videos', () => {
		expect(runs('aabaa')).toEqual(['0-5']);
	});

	it('never stacks videos without a channel', () => {
		expect(runs('----')).toEqual(['0-4']);
	});
});
