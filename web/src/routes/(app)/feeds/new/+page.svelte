<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { page } from '$app/state';
	import FeedEditor from '#lib/components/FeedEditor.svelte';

	/** `?filter=` carries the quick filters from the home page ("Save as feed"). */
	function initialFilter() {
		try {
			return JSON.parse(page.url.searchParams.get('filter') ?? '{}');
		} catch {
			return {};
		}
	}
</script>

<svelte:head><title>{m.feeds.newTitle}</title></svelte:head>

<FeedEditor initial={initialFilter()} />
