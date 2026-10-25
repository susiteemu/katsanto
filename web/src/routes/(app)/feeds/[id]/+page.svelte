<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import { api, unwrap, type Schemas } from '#lib/api/client.js';
	import Alert from '#lib/components/Alert.svelte';
	import FeedEditor from '#lib/components/FeedEditor.svelte';

	let feed = $state<Schemas['FeedInfo'] | null>(null);
	let missing = $state(false);

	onMount(async () => {
		const id = Number(page.params.id);
		feed = (await unwrap(api.GET('/api/v1/feeds'))).find((f) => f.id === id) ?? null;
		missing = !feed;
	});
</script>

<svelte:head><title>{m.feeds.editTitle}</title></svelte:head>

{#if feed}
	<FeedEditor {feed} />
{:else if missing}
	<Alert>{m.feeds.missing}</Alert>
{/if}
