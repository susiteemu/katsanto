<script lang="ts">
	import { onMount } from 'svelte';
	import type { Schemas } from '#lib/api/client.js';
	import { CATEGORY_COLORS, type Category, type SegmentAction } from '#lib/community.svelte.js';
	import { m } from '#lib/i18n/index.js';

	type Segment = Schemas['Segment'];
	/** The player's media element and the skin (for timeline markers). */
	type Media = Pick<
		HTMLMediaElement,
		'currentTime' | 'duration' | 'addEventListener' | 'removeEventListener'
	>;

	interface Props {
		media: () => Media | undefined;
		skin: () => HTMLElement | undefined;
		segments: Segment[];
		actions: Record<Category, SegmentAction>;
	}

	let { media, skin, segments, actions }: Props = $props();

	const action = (s: Segment): SegmentAction => actions[s.category as Category] ?? 'off';
	/** Ranges to act on; highlights are points and handled separately. */
	const ranges = $derived(
		segments.filter((s) => s.action === 'skip' && action(s) !== 'off' && s.end_s > s.start_s)
	);
	const highlight = $derived(
		segments.find((s) => s.action === 'poi' && action(s) === 'button') ?? null
	);
	const marked = $derived(segments.filter((s) => action(s) !== 'off'));

	let time = $state(0);
	/** Segments the user chose to watch after all ("Undo"). */
	let undone = $state(new Set<string>());
	let notice = $state<{ segment: Segment; from: number } | null>(null);
	let noticeTimer: ReturnType<typeof setTimeout> | undefined;
	let highlightUsed = $state(false);

	const name = (s: Segment) => m.community.categories[s.category as Category] ?? s.category;
	/** The segment playback is in, with its "skip" button (button mode). */
	const offered = $derived(
		ranges.find(
			(s) =>
				action(s) === 'button' && !undone.has(s.uuid) && time >= s.start_s && time < s.end_s - 0.5
		) ?? null
	);

	function onTime() {
		const el = media();
		if (!el) return;
		time = el.currentTime;
		const auto = ranges.find(
			(s) =>
				action(s) === 'skip' && !undone.has(s.uuid) && time >= s.start_s && time < s.end_s - 0.5
		);
		if (auto) skip(auto, true);
	}

	function skip(s: Segment, automatic: boolean) {
		const el = media();
		if (!el) return;
		const from = el.currentTime;
		el.currentTime = Math.min(s.end_s, el.duration || s.end_s);
		if (!automatic) return;
		notice = { segment: s, from };
		clearTimeout(noticeTimer);
		noticeTimer = setTimeout(() => (notice = null), 6000);
	}

	function undo() {
		if (!notice) return;
		undone = new Set([...undone, notice.segment.uuid]);
		const el = media();
		if (el) el.currentTime = notice.from;
		notice = null;
	}

	function jumpToHighlight() {
		const el = media();
		if (!el || !highlight) return;
		el.currentTime = highlight.start_s;
		highlightUsed = true;
	}

	onMount(() => {
		const el = media();
		el?.addEventListener('timeupdate', onTime);
		el?.addEventListener('durationchange', drawMarkers);
		return () => {
			el?.removeEventListener('timeupdate', onTime);
			el?.removeEventListener('durationchange', drawMarkers);
			clearTimeout(noticeTimer);
			skin()?.shadowRoot?.querySelector('.k-segments')?.remove();
		};
	});

	$effect(() => {
		void marked;
		drawMarkers();
	});

	/**
	 * Segments as colored ranges on the skin's time slider. The skin renders into an
	 * (open) shadow root, so the layer goes in there; without one there are no markers.
	 */
	function drawMarkers() {
		const slider = skin()?.shadowRoot?.querySelector('media-time-slider');
		const duration = media()?.duration;
		if (!(slider instanceof HTMLElement) || !duration || !Number.isFinite(duration)) return;
		slider.querySelector('.k-segments')?.remove();
		if (!marked.length) return;
		const layer = document.createElement('div');
		layer.className = 'k-segments';
		layer.setAttribute('aria-hidden', 'true');
		Object.assign(layer.style, {
			position: 'absolute',
			insetInline: '0',
			top: '50%',
			height: '4px',
			transform: 'translateY(-50%)',
			pointerEvents: 'none',
			zIndex: '1'
		});
		for (const s of marked) {
			const span = document.createElement('span');
			const left = (s.start_s / duration) * 100;
			const width = Math.max(((s.end_s - s.start_s) / duration) * 100, 0.4);
			Object.assign(span.style, {
				position: 'absolute',
				left: `${left}%`,
				width: `${width}%`,
				height: '100%',
				background: CATEGORY_COLORS[s.category as Category] ?? '#888',
				opacity: '0.85'
			});
			span.title = name(s);
			layer.append(span);
		}
		if (getComputedStyle(slider).position === 'static') slider.style.position = 'relative';
		slider.append(layer);
	}
</script>

<div class="pointer-events-none absolute right-3 bottom-20 z-10 flex flex-col items-end gap-2">
	{#if notice}
		<div
			class="pointer-events-auto flex items-center gap-3 rounded-full bg-black/80 px-4 py-2 text-sm text-white"
			role="status"
		>
			{m.community.skipped(name(notice.segment))}
			<button class="font-semibold text-white underline" onclick={undo}>{m.community.undo}</button>
		</div>
	{/if}
	{#if offered}
		<button
			class="pointer-events-auto rounded-full bg-black/80 px-4 py-2 text-sm font-medium text-white hover:bg-black"
			onclick={() => skip(offered, false)}
		>
			{m.community.skip(name(offered))} ⏭
		</button>
	{/if}
	{#if highlight && !highlightUsed && time < Math.min(highlight.start_s, 30)}
		<button
			class="pointer-events-auto rounded-full bg-black/80 px-4 py-2 text-sm font-medium text-white hover:bg-black"
			onclick={jumpToHighlight}
		>
			{m.community.jumpToHighlight} ⏩
		</button>
	{/if}
</div>
