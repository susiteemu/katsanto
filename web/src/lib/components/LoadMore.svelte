<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	/**
	 * Infinite scroll: calls `load` when scrolled into view (with a button
	 * fallback, also for keyboard users).
	 */
	let { load, loading = false }: { load: () => unknown; loading?: boolean } = $props();
	let el: HTMLDivElement;

	$effect(() => {
		const io = new IntersectionObserver(
			(entries) => {
				if (entries.some((e) => e.isIntersecting) && !loading) load();
			},
			{ rootMargin: '600px' }
		);
		io.observe(el);
		return () => io.disconnect();
	});
</script>

<div bind:this={el} class="flex justify-center py-6">
	{#if loading}
		<span
			class="size-6 animate-spin rounded-full border-2 border-muted border-t-transparent"
			role="status"
			aria-label={m.nav.loadingMore}
		></span>
	{:else}
		<button class="text-sm text-muted underline hover:text-fg" onclick={() => load()}
			>{m.nav.loadMore}</button
		>
	{/if}
</div>
