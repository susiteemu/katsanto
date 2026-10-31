/** This many consecutive videos from one channel become a stack. */
export const MIN_STACK = 4;

/** Part of a list: `items[start..end]`. */
export interface Run {
	start: number;
	end: number;
	/** At least `MIN_STACK` videos from one channel. */
	stack: boolean;
}

/**
 * Splits a feed into stacks of consecutive videos from one channel and the runs of
 * videos between them.
 */
export function channelRuns(items: { video: { channel?: { id: string } | null } }[]): Run[] {
	const runs: Run[] = [];
	const push = (start: number, end: number, stack: boolean) => {
		const last = runs.at(-1);
		if (!stack && last && !last.stack) last.end = end;
		else if (end > start) runs.push({ start, end, stack });
	};
	let start = 0;
	for (let i = 1; i <= items.length; i++) {
		const channel = items[i]?.video.channel?.id;
		if (channel && channel === items[start].video.channel?.id) continue;
		push(start, i, i - start >= MIN_STACK);
		start = i;
	}
	return runs;
}
