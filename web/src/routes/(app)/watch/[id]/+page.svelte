<script lang="ts">
	import { listName, m } from '#lib/i18n/index.js';
	import { page } from '$app/state';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import { ago, compact, duration } from '#lib/format.js';
	import { progressFor, type Progress } from '#lib/progress.js';
	import Alert from '#lib/components/Alert.svelte';
	import Description from '#lib/components/Description.svelte';
	import Player from '#lib/components/Player.svelte';
	import { branded } from '#lib/branding.svelte.js';
	import { communitySettings } from '#lib/community.svelte.js';
	import SubscribeButton from '#lib/components/SubscribeButton.svelte';
	import VideoCard from '#lib/components/VideoCard.svelte';
	import { goto } from '$app/navigation';
	import { saveToList } from '#lib/saveToList.svelte.js';

	/** Resume only when meaningfully into the video. */
	const MIN_RESUME_S = 30;

	const id = $derived(page.params.id!);

	let video = $state<Schemas['VideoResponse'] | null>(null);
	let info = $state<Schemas['VideoInfo'] | null>(null);
	let related = $state<Map<string, Progress>>(new Map());
	let startAt = $state(0);
	let resumed = $state(false);
	let error = $state('');
	let expanded = $state(false);
	let player = $state<ReturnType<typeof Player>>();
	let segments = $state<Schemas['Segment'][]>([]);
	const community = $derived(communitySettings());

	// ---- Play-list mode (`?list=<id>[&shuffle=1]`) ----
	const listId = $derived(Number(page.url.searchParams.get('list')) || null);
	const shuffle = $derived(page.url.searchParams.get('shuffle') === '1');
	let list = $state<Schemas['ListDetail'] | null>(null);
	let autoplay = $state(readAutoplay());

	function readAutoplay() {
		try {
			return localStorage.getItem('katsanto.autoplay') !== '0';
		} catch {
			return true;
		}
	}

	$effect(() => {
		try {
			localStorage.setItem('katsanto.autoplay', autoplay ? '1' : '0');
		} catch {
			// Not remembered.
		}
	});

	$effect(() => {
		const lid = listId;
		if (!lid) {
			list = null;
			return;
		}
		void unwrap(api.GET('/api/v1/lists/{id}', { params: { path: { id: lid } } })).then(
			(l) => (list = l),
			() => (list = null)
		);
	});

	/** Shuffled order, kept for the session so it doesn't change between videos. */
	function shuffledIds(lid: number, ids: string[]): string[] {
		const key = `katsanto.shuffle.${lid}`;
		try {
			const saved: string[] = JSON.parse(sessionStorage.getItem(key) ?? '[]');
			if (saved.length === ids.length && ids.every((i) => saved.includes(i))) return saved;
		} catch {
			// Fall through to a new order.
		}
		const order = [...ids];
		for (let i = order.length - 1; i > 0; i--) {
			const j = Math.floor(Math.random() * (i + 1));
			[order[i], order[j]] = [order[j], order[i]];
		}
		try {
			sessionStorage.setItem(key, JSON.stringify(order));
		} catch {
			// The order just won't persist.
		}
		return order;
	}

	const queue = $derived.by(() => {
		if (!list || !listId) return [];
		if (!shuffle) return list.items;
		const order = shuffledIds(
			listId,
			list.items.map((i) => i.video.id)
		);
		return order.map((vid) => list!.items.find((i) => i.video.id === vid)!);
	});
	const nextItem = $derived.by(() => {
		const idx = queue.findIndex((i) => i.video.id === id);
		return queue.slice(idx + 1).find((i) => !i.progress?.finished) ?? null;
	});

	// Manual next/previous (keys, media keys) step through the queue as is; autoplay
	// (`nextItem`) skips videos that are already watched.
	const queueIndex = $derived(queue.findIndex((i) => i.video.id === id));
	const prevItem = $derived(queueIndex > 0 ? queue[queueIndex - 1] : null);
	const followingItem = $derived(queue[queueIndex + 1] ?? null);
	const listUrl = (videoId: string) =>
		`/watch/${videoId}?list=${listId}${shuffle ? '&shuffle=1' : ''}`;

	function skipTo(item: (typeof queue)[number] | null) {
		if (item) void goto(listUrl(item.video.id));
	}

	/** Shift+N / Shift+P, as on YouTube. */
	function onkeydown(e: KeyboardEvent) {
		const t = e.target as HTMLElement;
		if (!listId || !e.shiftKey || e.ctrlKey || e.metaKey || e.altKey) return;
		if (/INPUT|TEXTAREA|SELECT/.test(t.tagName) || t.isContentEditable) return;
		if (e.key === 'N') skipTo(followingItem);
		else if (e.key === 'P') skipTo(prevItem);
		else return;
		e.preventDefault();
	}

	function playNext() {
		// Show the finished video as watched in the panel without reloading the list.
		const done = list?.items.find((i) => i.video.id === id);
		if (done) {
			const d = video?.details.duration_s ?? 0;
			done.progress = { video_id: id, position_s: d, duration_s: d, finished: true, updated_at: 0 };
		}
		if (!autoplay || !nextItem) return;
		skipTo(nextItem);
	}

	// Reload everything when navigating between videos.
	$effect(() => {
		void load(id, Number(page.url.searchParams.get('t')) || 0);
	});

	async function load(videoId: string, t: number) {
		video = null;
		segments = [];
		if (communitySettings().sponsorblock.enabled) {
			void unwrap(
				api.GET('/api/v1/videos/{video_id}/segments', { params: { path: { video_id: videoId } } })
			).then(
				(s) => videoId === id && (segments = s),
				() => {}
			);
		}
		info = null;
		error = '';
		expanded = false;
		resumed = false;
		window.scrollTo({ top: 0 });
		const infoReq = unwrap(
			api.GET('/api/v1/videos/{id}/info', { params: { path: { id: videoId } } })
		);
		try {
			const [v, progress] = await Promise.all([
				unwrap(api.GET('/api/v1/videos/{id}', { params: { path: { id: videoId } } })),
				progressFor([videoId])
			]);
			const p = progress.get(videoId);
			if (t > 0) startAt = t;
			else if (p && !p.finished && p.position_s >= MIN_RESUME_S) {
				startAt = p.position_s;
				resumed = true;
			} else startAt = 0;
			video = v;
		} catch (e) {
			error = errorMessage(e);
		}
		try {
			info = await infoReq;
			related = await progressFor(info.related.map((r) => r.id));
		} catch {
			// The player works without the extra info.
		}
	}

	function seek(seconds: number) {
		player?.seek(seconds);
		resumed = false;
		window.scrollTo({ top: 0, behavior: 'smooth' });
	}

	const youtubeTitle = $derived(info?.title ?? video?.details.title ?? '');
	const brand = $derived(branded({ id, title: youtubeTitle }));
	const title = $derived(youtubeTitle && brand.title);
	const descriptionLong = $derived(
		(info?.description ?? []).reduce((n, p) => n + p.text.length, 0) > 300
	);
</script>

<svelte:window {onkeydown} />
<svelte:head><title>{title ? `${title} · Katsanto` : 'Katsanto'}</title></svelte:head>

<div class="mx-auto grid max-w-[110rem] gap-6 lg:grid-cols-[minmax(0,1fr)_24rem] xl:gap-8">
	<div class="-mx-4 min-w-0 md:mx-0">
		{#if error}
			<div class="px-4 md:px-0"><Alert>{error}</Alert></div>
		{:else if video}
			{#key id}
				<Player
					bind:this={player}
					videoId={id}
					src={video.manifest_url}
					title={video.details.title}
					channel={video.details.channel.name}
					poster={video.details.thumbnail_url}
					captions={video.captions}
					chapters={info?.chapters ?? []}
					duration={video.details.duration_s}
					{startAt}
					onended={playNext}
					onnext={followingItem ? () => skipTo(followingItem) : undefined}
					onprevious={prevItem ? () => skipTo(prevItem) : undefined}
					{segments}
					segmentActions={community.sponsorblock.categories}
				/>
			{/key}
		{:else}
			<div class="aspect-video animate-pulse bg-surface-2 md:rounded-card"></div>
		{/if}

		<div class="mt-4 flex flex-col gap-4 px-4 md:px-0">
			{#if resumed}
				<div
					class="flex items-center justify-between gap-3 rounded-card bg-surface-2 px-4 py-2 text-sm"
				>
					<span>{m.watch.resumedAt(duration(startAt))}</span>
					<button class="font-medium text-accent hover:underline" onclick={() => seek(0)}
						>{m.watch.startOver}</button
					>
				</div>
			{/if}

			<h1
				class="text-xl leading-snug font-semibold"
				title={brand.original ? m.community.originalTitle(brand.original) : undefined}
			>
				{title}
			</h1>

			{#if info || video}
				{@const channel = info?.channel ?? video?.details.channel}
				<div class="flex flex-wrap items-center gap-3">
					{#if channel}
						<a href="/channel/{channel.id}" class="flex min-w-0 items-center gap-3">
							{#if info?.channel.avatar_url}
								<img
									src={info.channel.avatar_url}
									alt=""
									class="size-10 rounded-full bg-surface-2"
								/>
							{/if}
							<div class="min-w-0">
								<p class="truncate font-medium">{channel.name}</p>
								{#if info?.channel.subscriber_count != null}
									<p class="text-sm text-muted">
										{m.channel.subscribers(compact(info.channel.subscriber_count))}
									</p>
								{/if}
							</div>
						</a>
						<SubscribeButton channelId={channel.id} name={channel.name} />
						<button
							class="h-9 shrink-0 rounded-full bg-surface-2 px-4 text-sm font-medium hover:bg-border"
							onclick={() => saveToList.open(id, title)}>{m.watch.save}</button
						>
					{/if}
				</div>
			{/if}

			{#if info}
				<section class="rounded-card bg-surface-2 p-4">
					<p class="mb-2 text-sm font-medium">
						{[
							m.common.views(compact(info.view_count)),
							info.published_text ?? ago(info.published_at),
							info.like_count != null && m.watch.likes(compact(info.like_count))
						]
							.filter(Boolean)
							.join(' · ')}
					</p>
					<div class={expanded || !descriptionLong ? '' : 'line-clamp-4'}>
						<Description parts={info.description} videoId={id} onseek={seek} />
					</div>
					{#if descriptionLong}
						<button
							class="mt-2 text-sm font-medium hover:underline"
							onclick={() => (expanded = !expanded)}
						>
							{expanded ? m.watch.showLess : m.watch.showMore}
						</button>
					{/if}
				</section>

				{#if info.chapters.length}
					<section>
						<h2 class="mb-2 font-semibold">{m.watch.chapters}</h2>
						<ol class="flex flex-col">
							{#each info.chapters as c (c.start_s)}
								<li>
									<button
										class="flex w-full gap-3 rounded-card px-2 py-1.5 text-left text-sm hover:bg-surface-2"
										onclick={() => seek(c.start_s)}
									>
										<span class="w-14 shrink-0 text-accent tabular-nums">{duration(c.start_s)}</span
										>
										<span>{c.title}</span>
									</button>
								</li>
							{/each}
						</ol>
					</section>
				{/if}
			{/if}
		</div>
	</div>

	{#if list}
		<aside class="flex flex-col gap-3 lg:row-span-2" aria-label={m.watch.playingFromList}>
			<div
				class="flex flex-wrap items-center justify-between gap-2 rounded-card bg-surface-2 px-4 py-3"
			>
				<div class="min-w-0">
					<a href="/lists/{listId}" class="block truncate font-semibold hover:underline"
						>{listName(list.list)}</a
					>
					<p class="text-sm text-muted">
						{Math.max(1, queue.findIndex((i) => i.video.id === id) + 1)} / {queue.length}{shuffle
							? ` · ${m.watch.shuffled}`
							: ''}
					</p>
				</div>
				<label class="flex items-center gap-2 text-sm">
					<input type="checkbox" bind:checked={autoplay} class="size-4 accent-[var(--accent)]" />
					{m.watch.autoplay}
				</label>
			</div>
			<ol class="flex max-h-[70vh] flex-col gap-1 overflow-y-auto">
				{#each queue as item (item.video.id)}
					<li>
						<a
							href="/watch/{item.video.id}?list={listId}{shuffle ? '&shuffle=1' : ''}"
							aria-current={item.video.id === id ? 'true' : undefined}
							class="flex gap-3 rounded-card p-1.5 hover:bg-surface-2 aria-[current=true]:bg-accent/10"
						>
							<div
								class="relative aspect-video w-28 shrink-0 overflow-hidden rounded-md bg-surface-2"
							>
								{#if item.video.thumbnail_url}
									<img
										src={item.video.thumbnail_url}
										alt=""
										loading="lazy"
										class="size-full object-cover"
									/>
								{/if}
								{#if item.progress?.finished}
									<span
										class="absolute inset-0 grid place-items-center bg-black/50 text-xs font-medium text-white"
										>{m.video.watched}</span
									>
								{/if}
							</div>
							<div class="min-w-0 text-sm">
								<p class="line-clamp-2 font-medium">{item.video.title}</p>
								<p class="truncate text-muted">{item.video.channel?.name}</p>
							</div>
						</a>
					</li>
				{/each}
			</ol>
		</aside>
	{:else if info?.related.length}
		<aside class="flex flex-col gap-4" aria-label={m.watch.relatedVideos}>
			<h2 class="font-semibold lg:sr-only">{m.watch.related}</h2>
			{#each info.related as r (r.id)}
				<VideoCard video={r} progress={related.get(r.id)} layout="list" />
			{/each}
		</aside>
	{/if}
</div>
