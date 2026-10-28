<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import type { paths } from '#lib/api/schema.js';
	import { progressFor, type Progress } from '#lib/progress.js';
	import Alert from '#lib/components/Alert.svelte';
	import ChannelCard from '#lib/components/ChannelCard.svelte';
	import LoadMore from '#lib/components/LoadMore.svelte';
	import SearchBox from '#lib/components/SearchBox.svelte';
	import VideoCard from '#lib/components/VideoCard.svelte';
	import VideoGrid from '#lib/components/VideoGrid.svelte';

	type Query = NonNullable<paths['/api/v1/search']['get']['parameters']['query']>;

	const FILTERS = $derived({
		kind: [
			['video', m.search.videos],
			['channel', m.search.channels]
		],
		date: [
			['', m.search.anyTime],
			['today', m.search.today],
			['week', m.search.week],
			['month', m.search.month],
			['year', m.search.year]
		],
		length: [
			['', m.home.lengths.any],
			['short', m.home.lengths.short],
			['medium', m.home.lengths.medium],
			['long', m.home.lengths.long]
		],
		sort: [
			['', m.search.relevance],
			['date', m.search.uploadDate],
			['views', m.search.viewCount]
		]
	} as const);

	let items = $state<Schemas['SearchItem'][]>([]);
	let cursor = $state<string | null>(null);
	let corrected = $state<string | null>(null);
	let progress = $state<Map<string, Progress>>(new Map());
	let loading = $state(false);
	let error = $state('');
	let hideShorts = $state(false);

	const params = $derived(page.url.searchParams);
	const q = $derived(params.get('q') ?? '');
	const filter = (k: keyof typeof FILTERS) => params.get(k) ?? '';

	$effect(() => {
		if (q) void run(page.url.searchParams);
	});

	async function run(p: { get(name: string): string | null }) {
		loading = true;
		error = '';
		items = [];
		try {
			const query = { q: p.get('q') } as Record<string, string | null>;
			for (const k of ['kind', 'date', 'length', 'sort']) if (p.get(k)) query[k] = p.get(k);
			const res = await unwrap(api.GET('/api/v1/search', { params: { query: query as Query } }));
			items = res.items;
			cursor = res.cursor ?? null;
			corrected = res.corrected_query ?? null;
			progress = await progressFor(videoIds(res.items));
		} catch (e) {
			error = errorMessage(e);
		} finally {
			loading = false;
		}
	}

	async function more() {
		if (!cursor || loading) return;
		loading = true;
		try {
			const res = await unwrap(api.GET('/api/v1/search', { params: { query: { cursor } } }));
			// Continuation pages sometimes repeat earlier results.
			const seen = new Set(items.map((i) => i.id));
			items = [...items, ...res.items.filter((i) => !seen.has(i.id))];
			cursor = res.cursor ?? null;
			progress = new Map([...progress, ...(await progressFor(videoIds(res.items)))]);
		} catch (e) {
			error = errorMessage(e);
		} finally {
			loading = false;
		}
	}

	const videoIds = (list: Schemas['SearchItem'][]) =>
		list.filter((i) => i.type === 'video').map((i) => i.id);

	function setFilter(key: keyof typeof FILTERS, value: string) {
		const next = new URLSearchParams(page.url.search);
		if (value) next.set(key, value);
		else next.delete(key);
		void goto(`/search?${next}`, { reset: false });
	}

	const visible = $derived(items.filter((i) => !(hideShorts && i.type === 'video' && i.is_short)));
</script>

<svelte:head><title>{q ? m.search.titleFor(q) : m.search.title}</title></svelte:head>

<div class="mx-auto flex max-w-[110rem] flex-col gap-5">
	<h1 class="sr-only">{q || m.nav.search}</h1>
	<!-- On phones the header has no search box; search lives here. -->
	<div class="md:hidden"><SearchBox /></div>

	{#if q}
		<div class="flex flex-wrap items-center gap-2">
			{#each FILTERS.kind as [value, label] (value)}
				<button
					class="h-8 rounded-full px-3 text-sm font-medium {(filter('kind') || 'video') === value
						? 'bg-fg text-bg'
						: 'bg-surface-2 hover:bg-border'}"
					onclick={() => setFilter('kind', value === 'video' ? '' : value)}>{label}</button
				>
			{/each}
			<span class="mx-1 h-5 w-px bg-border" aria-hidden="true"></span>
			{#each ['date', 'length', 'sort'] as const as key (key)}
				<select
					value={filter(key)}
					onchange={(e) => setFilter(key, e.currentTarget.value)}
					aria-label={key === 'sort' ? m.search.sortBy : m.search.filterBy[key]}
					class="h-8 rounded-full border border-border bg-surface px-3 text-sm"
				>
					{#each FILTERS[key] as [value, label] (value)}
						<option {value}>{label}</option>
					{/each}
				</select>
			{/each}
			{#if (filter('kind') || 'video') === 'video'}
				<label class="ml-1 flex items-center gap-2 text-sm">
					<input type="checkbox" bind:checked={hideShorts} class="accent-[var(--accent)]" />
					{m.search.hideShorts}
				</label>
			{/if}
		</div>
	{/if}

	{#if corrected}
		<p class="text-sm text-muted">
			{m.search.showingFor}
			<a href="/search?q={encodeURIComponent(corrected)}" class="text-accent underline"
				>{corrected}</a
			>
		</p>
	{/if}
	{#if error}<Alert>{error}</Alert>{/if}

	{#if !q}
		<p class="py-16 text-center text-muted">{m.search.prompt}</p>
	{/if}

	{#if visible.length}<h2 class="sr-only">{m.search.results}</h2>{/if}
	<VideoGrid>
		{#each visible as item (item.id)}
			{#if item.type === 'video'}
				<VideoCard video={item} progress={progress.get(item.id)} showViews />
			{:else}
				<ChannelCard channel={item} />
			{/if}
		{/each}
	</VideoGrid>

	{#if q && (cursor || loading)}
		<LoadMore load={more} {loading} />
	{:else if q && !loading && items.length === 0 && !error}
		<p class="py-10 text-center text-muted">{m.search.noResults}</p>
	{/if}
</div>
