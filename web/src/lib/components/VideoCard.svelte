<script lang="ts">
	import type { Snippet } from 'svelte';
	import { m } from '#lib/i18n/index.js';
	import type { Schemas } from '#lib/api/client.js';
	import { ago, compact, duration, watchedFraction } from '#lib/format.js';
	import { api, errorMessage, unwrap } from '#lib/api/client.js';
	import type { Progress } from '#lib/progress.js';
	import { saveToList } from '#lib/saveToList.svelte.js';
	import { appearance } from '#lib/appearance.svelte.js';
	import { branded } from '#lib/branding.svelte.js';
	import { copyYouTubeLink } from '#lib/share.js';
	import Menu from './Menu.svelte';
	import { gridLayout } from './VideoGrid.svelte';

	interface Props {
		video: Schemas['VideoCard'];
		progress?: Progress | null;
		/** `list`: thumbnail beside the text. Default: the surrounding VideoGrid's layout. */
		layout?: 'grid' | 'list';
		/** Hide the channel name (on the channel's own page). */
		showChannel?: boolean;
		/**
		 * Show the view count. Only search results have a current one; elsewhere it is
		 * stored from the channel's feed and goes stale.
		 */
		showViews?: boolean;
		/** Show the actions menu; called after "Hide" so the list can drop the card. */
		onhide?: (id: string) => void;
		/** Extra items at the top of the actions menu. */
		menuItems?: Snippet;
	}

	let {
		video,
		progress = $bindable(null),
		layout: layoutProp,
		showChannel = true,
		showViews = false,
		onhide,
		menuItems
	}: Props = $props();

	async function act(fn: () => Promise<unknown>) {
		try {
			await fn();
		} catch (e) {
			alert(errorMessage(e));
		}
	}

	const hide = () =>
		act(async () => {
			await unwrap(
				api.POST('/api/v1/videos/{video_id}/hide', { params: { path: { video_id: video.id } } })
			);
			onhide?.(video.id);
		});

	const markWatched = () =>
		act(async () => {
			progress = await unwrap(
				api.POST('/api/v1/videos/{video_id}/watched', { params: { path: { video_id: video.id } } })
			);
		});

	let savedLater = $state(false);
	const watchLater = () =>
		act(async () => {
			await saveToList.watchLater(video.id);
			savedLater = true;
		});

	const markUnwatched = () =>
		act(async () => {
			await unwrap(
				api.DELETE('/api/v1/progress/{video_id}', { params: { path: { video_id: video.id } } })
			);
			progress = null;
		});

	const b = $derived(branded(video));
	/** A DeArrow thumbnail that failed to load falls back to YouTube's. */
	let thumbFailed = $state(false);
	const thumb = $derived(thumbFailed ? video.thumbnail_url : b.thumbnail_url);

	const contextLayout = gridLayout();
	const layout = $derived(layoutProp ?? contextLayout ?? 'grid');
	const show = $derived(appearance.settings.show);
	const watched = $derived(watchedFraction(progress));
	const meta = $derived(
		[
			showViews && show.views && video.view_count != null
				? m.common.views(compact(video.view_count))
				: null,
			show.dates ? (video.published_text ?? ago(video.published_at)) : null
		]
			.filter(Boolean)
			.join(' · ')
	);
</script>

<article class="group relative flex gap-3 {layout === 'grid' ? 'flex-col' : 'flex-row'}">
	<a
		href="/watch/{video.id}"
		class="relative block shrink-0 overflow-hidden rounded-card bg-surface-2 {layout === 'grid'
			? 'w-full'
			: 'w-40 sm:w-44'} {video.is_short ? 'aspect-[9/16]' : 'aspect-video'}"
		aria-label={b.title}
	>
		{#if thumb}
			<img
				src={thumb}
				alt=""
				onerror={() => (thumbFailed = true)}
				loading="lazy"
				class="size-full object-cover transition duration-300 group-hover:scale-[1.03]"
			/>
		{/if}
		{#if video.is_live}
			<span
				class="absolute right-1.5 bottom-1.5 rounded bg-danger px-1.5 py-0.5 text-xs font-medium text-danger-fg"
				>{m.video.live}</span
			>
		{:else if video.is_upcoming}
			<span
				class="absolute right-1.5 bottom-1.5 rounded bg-black/80 px-1.5 py-0.5 text-xs text-white"
				>{m.video.upcoming}</span
			>
		{:else if video.duration_s && show.durations}
			<span
				class="absolute right-1.5 bottom-1.5 rounded bg-black/80 px-1.5 py-0.5 text-xs font-medium text-white tabular-nums"
				>{duration(video.duration_s)}</span
			>
		{/if}
		{#if watched !== null}
			<div class="absolute inset-x-0 bottom-0 h-1 bg-white/30" aria-hidden="true">
				<div class="h-full bg-accent" style="width: {watched * 100}%"></div>
			</div>
			<span class="sr-only"
				>{watched >= 1 ? m.video.watched : m.video.percentWatched(Math.round(watched * 100))}</span
			>
		{/if}
	</a>
	<div class="flex min-w-0 gap-3">
		{#if layout === 'grid' && showChannel && show.avatars && video.channel?.avatar_url}
			<!-- Duplicates the channel name link below, so it is hidden from assistive tech. -->
			<a href="/channel/{video.channel.id}" class="shrink-0" tabindex="-1" aria-hidden="true">
				<img
					src={video.channel.avatar_url}
					alt=""
					class="size-9 rounded-full bg-surface-2"
					loading="lazy"
				/>
			</a>
		{/if}
		<div class="min-w-0">
			<h3 class="line-clamp-2 leading-snug font-medium {layout === 'list' ? 'text-sm' : ''}">
				<a
					href="/watch/{video.id}"
					class="hover:underline"
					title={b.original ? `${b.title}\n${m.community.originalTitle(b.original)}` : b.title}
					>{b.title}</a
				>
			</h3>
			{#if showChannel && video.channel}
				<a
					href="/channel/{video.channel.id}"
					class="mt-0.5 block truncate text-sm text-muted hover:text-fg">{video.channel.name}</a
				>
			{/if}
			{#if meta}<p class="text-sm text-muted">{meta}</p>{/if}
		</div>
		<div class="-mr-2 ml-auto shrink-0">
			<Menu label={m.video.actions(b.title)}>
				{@render menuItems?.()}
				<button role="menuitem" onclick={watchLater} disabled={savedLater}
					>{savedLater ? m.video.savedLater : m.video.saveLater}</button
				>
				<button role="menuitem" onclick={() => saveToList.open(video.id, video.title)}
					>{m.video.saveToList}</button
				>
				<button role="menuitem" onclick={() => copyYouTubeLink(video.id)}>{m.video.copyLink}</button
				>
				{#if onhide}
					{#if progress}
						<button role="menuitem" onclick={markUnwatched}>{m.video.markUnwatched}</button>
					{/if}
					{#if !progress?.finished}
						<button role="menuitem" onclick={markWatched}>{m.video.markWatched}</button>
					{/if}
					<button role="menuitem" onclick={hide}>{m.video.hide}</button>
				{/if}
			</Menu>
		</div>
	</div>
</article>
