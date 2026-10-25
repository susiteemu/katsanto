<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import { compact } from '#lib/format.js';
	import { progressFor, type Progress } from '#lib/progress.js';
	import Alert from '#lib/components/Alert.svelte';
	import LoadMore from '#lib/components/LoadMore.svelte';
	import SubscribeButton from '#lib/components/SubscribeButton.svelte';
	import VideoCard from '#lib/components/VideoCard.svelte';
	import VideoGrid from '#lib/components/VideoGrid.svelte';

	type Tab = 'videos' | 'shorts' | 'live';

	const id = $derived(page.params.id!);
	const tab = $derived((page.url.searchParams.get('tab') as Tab | null) ?? 'videos');

	let channel = $state<Schemas['ChannelInfo'] | null>(null);
	let videos = $state<Schemas['VideoCard'][]>([]);
	let cursor = $state<string | null>(null);
	let progress = $state<Map<string, Progress>>(new Map());
	let loading = $state(false);
	let error = $state('');

	$effect(() => {
		void load(id, tab);
	});

	async function load(channelId: string, t: Tab) {
		error = '';
		// `@handle` links: resolve to the channel id first.
		if (channelId.startsWith('@')) {
			try {
				const r = await unwrap(
					api.GET('/api/v1/resolve', {
						params: { query: { url: `https://www.youtube.com/${channelId}` } }
					})
				);
				if (r.type === 'channel') return goto(`/channel/${r.id}`, { replace: true });
			} catch (e) {
				error = errorMessage(e);
			}
			return;
		}
		loading = true;
		videos = [];
		try {
			const res = await unwrap(
				api.GET('/api/v1/channels/{id}', { params: { path: { id: channelId }, query: { tab: t } } })
			);
			channel = res.channel;
			videos = res.videos.items;
			cursor = res.videos.cursor ?? null;
			progress = await progressFor(videos.map((v) => v.id));
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
			const res = await unwrap(
				api.GET('/api/v1/channels/{id}/videos', { params: { path: { id }, query: { cursor } } })
			);
			videos = [...videos, ...res.items];
			cursor = res.cursor ?? null;
			const p = await progressFor(res.items.map((v) => v.id));
			progress = new Map([...progress, ...p]);
		} catch (e) {
			error = errorMessage(e);
		} finally {
			loading = false;
		}
	}

	const tabs = $derived<{ id: Tab; label: string }[]>([
		{ id: 'videos', label: m.channelPage.videos },
		...(channel?.has_shorts ? [{ id: 'shorts' as Tab, label: m.channelPage.shorts }] : []),
		...(channel?.has_live ? [{ id: 'live' as Tab, label: m.channelPage.live }] : [])
	]);
</script>

<svelte:head
	><title>{channel ? `${channel.name} · Katsanto` : m.channelPage.title}</title></svelte:head
>

<div class="mx-auto flex max-w-[110rem] flex-col gap-6">
	{#if channel}
		{#if channel.banner_url}
			<img
				src={channel.banner_url}
				alt=""
				class="-mx-4 aspect-[6/1] w-[calc(100%+2rem)] max-w-none object-cover md:mx-0 md:w-full md:rounded-card"
			/>
		{/if}
		<header class="flex flex-wrap items-center gap-4">
			{#if channel.avatar_url}
				<img src={channel.avatar_url} alt="" class="size-16 rounded-full bg-surface-2 sm:size-20" />
			{/if}
			<div class="min-w-0">
				<h1 class="truncate text-2xl font-semibold">{channel.name}</h1>
				<p class="text-sm text-muted">
					{[
						channel.handle,
						channel.subscriber_count != null &&
							m.channel.subscribers(compact(channel.subscriber_count)),
						channel.video_count != null && m.channelPage.videoCount(compact(channel.video_count))
					]
						.filter(Boolean)
						.join(' · ')}
				</p>
				{#if channel.description}
					<p class="mt-1 line-clamp-2 max-w-3xl text-sm text-muted">{channel.description}</p>
				{/if}
			</div>
			<div class="sm:ml-auto"><SubscribeButton channelId={channel.id} name={channel.name} /></div>
		</header>

		<nav class="flex gap-1 border-b border-border" aria-label={m.channelPage.sections}>
			{#each tabs as t (t.id)}
				<a
					href="?tab={t.id}"
					aria-current={tab === t.id ? 'page' : undefined}
					class="border-b-2 border-transparent px-4 py-2 text-sm font-medium text-muted hover:text-fg aria-[current=page]:border-accent aria-[current=page]:text-fg"
					>{t.label}</a
				>
			{/each}
		</nav>
	{/if}

	{#if error}<Alert>{error}</Alert>{/if}

	<h2 class="sr-only">{tabs.find((t) => t.id === tab)?.label}</h2>
	{#if tab === 'shorts'}
		<div class="grid grid-cols-2 gap-x-4 gap-y-8 sm:grid-cols-3 lg:grid-cols-5 xl:grid-cols-6">
			{#each videos as v (v.id)}
				<VideoCard video={v} progress={progress.get(v.id)} showChannel={false} layout="grid" />
			{/each}
		</div>
	{:else}
		<VideoGrid>
			{#each videos as v (v.id)}
				<VideoCard video={v} progress={progress.get(v.id)} showChannel={false} />
			{/each}
		</VideoGrid>
	{/if}

	{#if cursor}
		<LoadMore load={more} {loading} />
	{:else if loading}
		<LoadMore load={() => {}} loading />
	{:else if channel && videos.length === 0}
		<p class="text-center text-muted">{m.channelPage.empty}</p>
	{/if}
</div>
