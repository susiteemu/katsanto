<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	/**
	 * Pull down at the top of the page to call `refresh`. Only in the installed app:
	 * in a browser tab the browser's own pull-to-refresh reloads the page.
	 */
	let { refresh }: { refresh: () => Promise<unknown> } = $props();

	/** How far (after damping) the pull must go to refresh. */
	const THRESHOLD = 64;
	const MAX = 96;

	let pull = $state(0);
	let refreshing = $state(false);

	$effect(() => {
		const standalone =
			matchMedia('(display-mode: standalone)').matches ||
			(navigator as { standalone?: boolean }).standalone === true;
		if (!standalone) return;

		let start: { x: number; y: number } | null = null;

		const onStart = (e: TouchEvent) => {
			start =
				!refreshing && e.touches.length === 1 && scrollY <= 0
					? { x: e.touches[0].clientX, y: e.touches[0].clientY }
					: null;
		};
		const onMove = (e: TouchEvent) => {
			if (!start) return;
			const dx = e.touches[0].clientX - start.x;
			const dy = e.touches[0].clientY - start.y;
			// A sideways swipe (the feed chips, Continue watching) or scrolling down isn't a pull.
			if (pull === 0 && (Math.abs(dx) > Math.abs(dy) || dy < 0)) {
				start = null;
				return;
			}
			pull = Math.min(MAX, Math.max(0, dy * 0.5));
		};
		const onEnd = async () => {
			if (!start) return;
			start = null;
			if (pull < THRESHOLD) {
				pull = 0;
				return;
			}
			refreshing = true;
			pull = THRESHOLD;
			try {
				await refresh();
			} finally {
				refreshing = false;
				pull = 0;
			}
		};

		addEventListener('touchstart', onStart, { passive: true });
		addEventListener('touchmove', onMove, { passive: true });
		addEventListener('touchend', onEnd);
		addEventListener('touchcancel', onEnd);
		return () => {
			removeEventListener('touchstart', onStart);
			removeEventListener('touchmove', onMove);
			removeEventListener('touchend', onEnd);
			removeEventListener('touchcancel', onEnd);
		};
	});
</script>

{#if pull > 0 || refreshing}
	<div
		class="pointer-events-none fixed inset-x-0 top-[env(safe-area-inset-top)] z-20 flex justify-center"
		style:transform="translateY({pull - 40}px)"
		style:opacity={Math.min(1, pull / THRESHOLD)}
	>
		<div class="grid size-10 place-items-center rounded-full bg-surface shadow-card">
			<span
				class={[
					'size-5 rounded-full border-2 border-t-transparent',
					pull >= THRESHOLD ? 'border-accent' : 'border-muted',
					refreshing && 'animate-spin'
				]}
				style:rotate={refreshing ? undefined : `${pull * 4}deg`}
				role={refreshing ? 'status' : undefined}
				aria-label={refreshing ? m.nav.refreshing : undefined}
			></span>
		</div>
	</div>
{/if}
