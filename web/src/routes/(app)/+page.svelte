<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { goto } from '$app/navigation';
	import { page, updated } from '$app/state';
	import { onMount } from 'svelte';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import { subscriptions } from '#lib/subscriptions.svelte.js';
	import Alert from '#lib/components/Alert.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import LoadMore from '#lib/components/LoadMore.svelte';
	import PullToRefresh from '#lib/components/PullToRefresh.svelte';
	import VideoCard from '#lib/components/VideoCard.svelte';
	import VideoGrid from '#lib/components/VideoGrid.svelte';

	type Filter = Schemas['FeedFilter'];
	type Watched = Filter['watched'];

	const PREFS_KEY = 'katsanto.feedPrefs';
	const LENGTHS = [
		{ id: 'any', min: null, max: null },
		{ id: 'short', min: null, max: 240 },
		{ id: 'medium', min: 240, max: 1200 },
		{ id: 'long', min: 1200, max: null }
	] as const;
	const WATCHED: Watched[] = ['all', 'hide_finished', 'only_unwatched', 'in_progress'];

	interface Quick {
		text: string;
		shorts: boolean;
		watched: Watched;
		length: (typeof LENGTHS)[number]['id'];
	}

	let feeds = $state<Schemas['FeedInfo'][]>([]);
	let continueItems = $state<Schemas['ContinueItem'][]>([]);
	let items = $state<Schemas['FeedItem'][]>([]);
	let cursor = $state<string | null>(null);
	let loading = $state(true);
	let error = $state('');
	let fresh = $state(0);
	let quick = $state<Quick>(readPrefs());

	const feedId = $derived(Number(page.url.searchParams.get('feed')) || null);
	const current = $derived(feeds.find((f) => f.id === feedId) ?? null);

	function readPrefs(): Quick {
		const defaults: Quick = { text: '', shorts: false, watched: 'all', length: 'any' };
		try {
			const saved = JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}');
			return { ...defaults, shorts: !!saved.shorts, watched: saved.watched ?? 'all' };
		} catch {
			return defaults;
		}
	}

	/** The quick filter bar as a feed filter. */
	function quickFilter(q: Quick): Filter {
		const len = LENGTHS.find((l) => l.id === q.length)!;
		return {
			tag_ids: [],
			channels: [],
			exclude_channels: [],
			title_any: q.text.trim() ? [q.text.trim()] : [],
			title_none: [],
			title_regex: null,
			min_duration_s: len.min,
			max_duration_s: len.max,
			shorts: q.shorts,
			live: true,
			watched: q.watched,
			published_within_days: null
		};
	}

	const quickActive = $derived(quick.text.trim() !== '' || quick.length !== 'any');

	onMount(() => {
		void subscriptions.load();
		void loadExtras();

		const events = new EventSource('/api/v1/events');
		events.addEventListener('feed', (e) => {
			// With nothing on screen there's nothing to disrupt: just show them.
			if (items.length === 0) void load();
			else fresh += JSON.parse((e as MessageEvent).data).count ?? 0;
		});
		return () => events.close();
	});

	// Reload when the selected feed or the quick filters change.
	let textTimer: ReturnType<typeof setTimeout>;
	$effect(() => {
		const q = { ...quick };
		const id = feedId;
		try {
			localStorage.setItem(PREFS_KEY, JSON.stringify({ shorts: q.shorts, watched: q.watched }));
		} catch {
			// Preferences just won't persist.
		}
		clearTimeout(textTimer);
		// Debounce typing in the title filter.
		textTimer = setTimeout(() => void load(false, id, q), q.text ? 300 : 0);
	});

	async function load(more = false, id = feedId, q = quick) {
		loading = true;
		error = '';
		const after = more ? (cursor ?? undefined) : undefined;
		try {
			const res = id
				? await unwrap(
						api.GET('/api/v1/feeds/{id}/videos', {
							params: { path: { id }, query: { cursor: after } }
						})
					)
				: await unwrap(
						api.POST('/api/v1/feeds/preview', { body: { filter: quickFilter(q), cursor: after } })
					);
			items = more ? [...items, ...res.items] : res.items;
			cursor = res.cursor ?? null;
			if (!more) fresh = 0;
		} catch (e) {
			error = errorMessage(e);
		} finally {
			loading = false;
		}
	}

	/** The feed chips and Continue watching. */
	function loadExtras() {
		return Promise.allSettled([
			unwrap(api.GET('/api/v1/feeds')).then((f) => (feeds = f)),
			unwrap(api.GET('/api/v1/continue-watching', { params: { query: { limit: 12 } } })).then(
				(c) => (continueItems = c)
			)
		]);
	}

	async function refresh() {
		// A new version was deployed: reloading gets it along with the feed.
		if (await updated.check()) return location.reload();
		await Promise.all([load(), loadExtras()]);
	}

	async function removeContinue(id: string) {
		continueItems = continueItems.filter((i) => i.video.id !== id);
		await api.DELETE('/api/v1/progress/{video_id}', { params: { path: { video_id: id } } });
	}

	function showNew() {
		window.scrollTo({ top: 0, behavior: 'smooth' });
		void load();
	}

	function saveAsFeed() {
		void goto(`/feeds/new?filter=${encodeURIComponent(JSON.stringify(quickFilter(quick)))}`);
	}

	const noSubscriptions = $derived(subscriptions.ids.size === 0);
</script>

<svelte:head><title>{m.home.title}</title></svelte:head>

<PullToRefresh {refresh} />

<div class="mx-auto flex max-w-[110rem] flex-col gap-6">
	<h1 class="sr-only">{current?.name ?? m.nav.home}</h1>
	<nav class="-mx-4 flex gap-2 overflow-x-auto px-4 pb-1 md:mx-0 md:px-0" aria-label={m.home.feeds}>
		<a
			href="/"
			aria-current={feedId === null ? 'page' : undefined}
			class="h-9 shrink-0 rounded-full bg-surface-2 px-4 pt-[0.45rem] text-sm font-medium hover:bg-border aria-[current=page]:bg-fg aria-[current=page]:text-bg"
			>{m.home.all}</a
		>
		{#each feeds as f (f.id)}
			<a
				href="/?feed={f.id}"
				aria-current={feedId === f.id ? 'page' : undefined}
				class="h-9 shrink-0 rounded-full bg-surface-2 px-4 pt-[0.45rem] text-sm font-medium whitespace-nowrap hover:bg-border aria-[current=page]:bg-fg aria-[current=page]:text-bg"
				>{f.icon ? `${f.icon} ` : ''}{f.name}</a
			>
		{/each}
		<a
			href="/feeds/new"
			class="grid size-9 shrink-0 place-items-center rounded-full border border-dashed border-border text-muted hover:text-fg"
			aria-label={m.home.newFeed}
			title={m.home.newFeed}><Icon name="plus" size={16} /></a
		>
		{#if feeds.length}
			<a href="/feeds" class="shrink-0 self-center px-2 text-sm text-muted hover:text-fg"
				>{m.home.manage}</a
			>
		{/if}
	</nav>

	{#if feedId === null && continueItems.length}
		<section aria-labelledby="continue">
			<h2 id="continue" class="mb-3 text-lg font-semibold">{m.home.continueWatching}</h2>
			<div class="-mx-4 flex snap-x gap-4 overflow-x-auto px-4 pb-2 md:mx-0 md:px-0">
				{#each continueItems as item (item.video.id)}
					<div class="group/item relative w-64 shrink-0 snap-start sm:w-72">
						<VideoCard
							video={item.video}
							progress={{
								video_id: item.video.id,
								position_s: item.position_s,
								duration_s: item.duration_s,
								finished: false,
								updated_at: item.updated_at
							}}
						>
							{#snippet menuItems()}
								<button role="menuitem" onclick={() => removeContinue(item.video.id)}
									>{m.home.removeFromContinue}</button
								>
							{/snippet}
						</VideoCard>
						<button
							class="absolute top-2 right-2 rounded-full bg-black/70 p-1.5 text-white opacity-0 transition group-hover/item:opacity-100 focus:opacity-100"
							onclick={() => removeContinue(item.video.id)}
							aria-label={m.home.removeContinue(item.video.title)}
							title={m.common.remove}><Icon name="trash" size={16} /></button
						>
					</div>
				{/each}
			</div>
		</section>
	{/if}

	<section aria-label={current?.name ?? m.home.allSubscriptions} class="flex flex-col gap-4">
		{#if current}
			<div class="flex items-center justify-between gap-3">
				<h2 class="text-lg font-semibold">{current.icon} {current.name}</h2>
				<a href="/feeds/{current.id}" class="text-sm text-muted hover:text-fg">{m.home.editFeed}</a>
			</div>
		{:else}
			<h2 class="sr-only">{m.home.allSubscriptions}</h2>
			<div class="flex flex-wrap items-center gap-2">
				<input
					type="search"
					bind:value={quick.text}
					placeholder={m.home.filterTitles}
					aria-label={m.home.filterTitles}
					class="h-9 w-48 rounded-full border border-border bg-surface px-4 text-sm"
				/>
				<select
					bind:value={quick.length}
					aria-label={m.home.length}
					class="h-9 rounded-full border border-border bg-surface px-3 text-sm"
				>
					{#each LENGTHS as l (l.id)}<option value={l.id}>{m.home.lengths[l.id]}</option>{/each}
				</select>
				<select
					bind:value={quick.watched}
					aria-label={m.home.watched}
					class="h-9 rounded-full border border-border bg-surface px-3 text-sm"
				>
					{#each WATCHED as w (w)}<option value={w}>{m.home.watchedOptions[w]}</option>{/each}
				</select>
				<label class="flex items-center gap-2 px-1 text-sm">
					<input
						type="checkbox"
						bind:checked={quick.shorts}
						class="size-4 accent-[var(--accent)]"
					/>
					{m.home.shorts}
				</label>
				{#if quickActive}
					<button
						class="ml-auto text-sm font-medium text-accent hover:underline"
						onclick={saveAsFeed}>{m.home.saveAsFeed}</button
					>
				{/if}
			</div>
		{/if}

		{#if fresh > 0}
			<button
				class="sticky top-16 z-10 self-center rounded-full bg-accent px-4 py-2 text-sm font-medium text-accent-fg shadow-card"
				onclick={showNew}
			>
				{m.home.fresh(fresh)}
			</button>
		{/if}

		{#if error}<Alert>{error}</Alert>{/if}

		{#if !loading && items.length === 0 && !error}
			<div class="rounded-card border border-dashed border-border p-10 text-center text-muted">
				{#if noSubscriptions}
					<p class="mb-3">{m.home.noSubscriptions}</p>
					<a href="/search" class="text-accent underline">{m.home.findChannels}</a>
				{:else if current || quickActive}
					<p>{m.home.noMatches}</p>
				{:else}
					<p>{m.home.empty}</p>
				{/if}
			</div>
		{/if}

		<VideoGrid>
			{#each items as item, i (item.video.id)}
				<VideoCard
					video={item.video}
					bind:progress={items[i].progress}
					onhide={(id) => (items = items.filter((x) => x.video.id !== id))}
				/>
			{/each}
		</VideoGrid>

		{#if cursor || (loading && items.length === 0)}
			<LoadMore load={() => load(true)} {loading} />
		{/if}
	</section>
</div>
