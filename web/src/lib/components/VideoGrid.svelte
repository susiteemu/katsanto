<script lang="ts" module>
	import { getContext } from 'svelte';

	const KEY = Symbol('video-grid-layout');

	/** The layout of the surrounding VideoGrid, for cards without an explicit one. */
	export function gridLayout(): 'grid' | 'list' | undefined {
		return getContext<() => 'grid' | 'list'>(KEY)?.();
	}
</script>

<script lang="ts">
	import { setContext, type Snippet } from 'svelte';
	import { appearance } from '#lib/appearance.svelte.js';

	/** Lays out video cards according to the user's layout and card size settings. */
	let { children }: { children: Snippet } = $props();

	const s = $derived(appearance.settings);
	setContext(KEY, () => s.layout);

	const COLUMNS = {
		small:
			'grid-cols-2 gap-x-3 gap-y-6 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 2xl:grid-cols-6',
		medium: 'grid-cols-1 gap-x-4 gap-y-8 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4',
		large: 'grid-cols-1 gap-x-6 gap-y-10 md:grid-cols-2 2xl:grid-cols-3'
	};
	const classes = $derived(
		s.layout === 'list' ? 'grid-cols-1 gap-x-8 gap-y-4 xl:grid-cols-2' : COLUMNS[s.cardSize]
	);
</script>

<div class="grid {classes}">
	{@render children()}
</div>
