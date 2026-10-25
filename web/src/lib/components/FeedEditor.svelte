<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { goto } from '$app/navigation';
	import { onMount, untrack } from 'svelte';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import Alert from './Alert.svelte';
	import Button from './Button.svelte';
	import Card from './Card.svelte';
	import Field from './Field.svelte';
	import KeywordInput from './KeywordInput.svelte';
	import TagChip from './TagChip.svelte';
	import VideoCard from './VideoCard.svelte';

	type Filter = Schemas['FeedFilter'];

	interface Props {
		/** Editing an existing feed; otherwise creating one. */
		feed?: Schemas['FeedInfo'] | null;
		/** Initial filter for a new feed (e.g. from the quick filter bar). */
		initial?: Partial<Filter>;
	}

	let { feed = null, initial = {} }: Props = $props();

	const EMPTY: Filter = {
		tag_ids: [],
		channels: [],
		exclude_channels: [],
		title_any: [],
		title_none: [],
		title_regex: null,
		min_duration_s: null,
		max_duration_s: null,
		shorts: false,
		live: true,
		watched: 'all',
		published_within_days: null
	};
	const ICONS = ['', '💻', '🎮', '🎵', '🎬', '📚', '🔬', '🍳', '⚽', '🛠️', '📰', '😂'];

	// The form edits a copy taken when it opens.
	const start = untrack(() => ({
		name: feed?.name ?? '',
		icon: feed?.icon ?? '',
		filter: { ...EMPTY, ...(feed?.filter ?? initial) }
	}));
	let name = $state(start.name);
	let icon = $state(start.icon);
	let filter = $state<Filter>(start.filter);
	// Minutes in the form, seconds in the filter.
	let minMinutes = $state(filter.min_duration_s != null ? String(filter.min_duration_s / 60) : '');
	let maxMinutes = $state(filter.max_duration_s != null ? String(filter.max_duration_s / 60) : '');
	let regex = $state(filter.title_regex ?? '');

	let tags = $state<Schemas['TagInfo'][]>([]);
	let subs = $state<Schemas['SubscriptionInfo'][]>([]);
	let preview = $state<Schemas['FeedItem'][]>([]);
	let previewError = $state('');
	let error = $state('');
	let busy = $state(false);
	let channelQuery = $state('');

	onMount(async () => {
		[tags, subs] = await Promise.all([
			unwrap(api.GET('/api/v1/tags')),
			unwrap(api.GET('/api/v1/subscriptions'))
		]);
	});

	const minutes = (s: string) => (s.trim() === '' ? null : Math.round(Number(s) * 60));

	/** The filter as it will be saved. */
	const current = $derived<Filter>({
		...filter,
		min_duration_s: minutes(minMinutes),
		max_duration_s: minutes(maxMinutes),
		title_regex: regex.trim() || null
	});

	// Live preview, debounced.
	$effect(() => {
		const f = $state.snapshot(current);
		const t = setTimeout(async () => {
			try {
				previewError = '';
				preview = (await unwrap(api.POST('/api/v1/feeds/preview', { body: { filter: f } }))).items;
			} catch (e) {
				previewError = errorMessage(e);
				preview = [];
			}
		}, 400);
		return () => clearTimeout(t);
	});

	function toggle(list: (number | string)[], value: number | string) {
		return list.includes(value) ? list.filter((v) => v !== value) : [...list, value];
	}

	const channelName = (id: string) => subs.find((s) => s.channel_id === id)?.name ?? id;
	const channelMatches = $derived(
		channelQuery.trim()
			? subs
					.filter((s) => s.name.toLowerCase().includes(channelQuery.trim().toLowerCase()))
					.slice(0, 8)
			: []
	);

	async function save(e: SubmitEvent) {
		e.preventDefault();
		busy = true;
		error = '';
		try {
			const body = { name, icon, filter: current };
			const saved = feed
				? await unwrap(api.PUT('/api/v1/feeds/{id}', { params: { path: { id: feed.id } }, body }))
				: await unwrap(api.POST('/api/v1/feeds', { body }));
			await goto(`/?feed=${saved.id}`);
		} catch (err) {
			error = errorMessage(err);
		} finally {
			busy = false;
		}
	}

	async function remove() {
		if (!feed || !confirm(m.feeds.confirmDeleteFeed(feed.name))) return;
		await unwrap(api.DELETE('/api/v1/feeds/{id}', { params: { path: { id: feed.id } } }));
		await goto('/');
	}
</script>

<div class="mx-auto grid max-w-[110rem] gap-8 xl:grid-cols-[34rem_minmax(0,1fr)]">
	<form class="flex flex-col gap-5" onsubmit={save}>
		<h1 class="text-2xl font-semibold">{feed ? m.feedEditor.edit : m.feedEditor.new}</h1>
		{#if error}<Alert>{error}</Alert>{/if}

		<Card>
			<div class="flex flex-col gap-4">
				<Field
					label={m.feedEditor.name}
					bind:value={name}
					required
					maxlength={40}
					placeholder={m.feedEditor.namePlaceholder}
				/>
				<fieldset>
					<legend class="mb-1.5 text-sm font-medium">{m.feedEditor.icon}</legend>
					<div class="flex flex-wrap gap-1.5">
						{#each ICONS as i (i)}
							<button
								type="button"
								class="grid size-9 place-items-center rounded-card border text-lg {icon === i
									? 'border-accent bg-accent/10'
									: 'border-border'}"
								aria-label={i || m.feedEditor.noIcon}
								aria-pressed={icon === i}
								onclick={() => (icon = i)}>{i || '—'}</button
							>
						{/each}
					</div>
				</fieldset>
			</div>
		</Card>

		<Card title={m.feedEditor.channels} description={m.feedEditor.channelsHint}>
			<div class="flex flex-col gap-4">
				<div>
					<p class="mb-1.5 text-sm font-medium">{m.feedEditor.taggedWith}</p>
					{#if tags.length}
						<div class="flex flex-wrap gap-1.5">
							{#each tags as t (t.id)}
								<TagChip
									name={t.name}
									color={t.color}
									selected={filter.tag_ids.includes(t.id)}
									onclick={() => (filter.tag_ids = toggle(filter.tag_ids, t.id) as number[])}
								/>
							{/each}
						</div>
					{:else}
						<p class="text-sm text-muted">
							{m.feedEditor.noTagsBefore}
							<a href="/subscriptions" class="text-accent underline">{m.feedEditor.noTagsLink}</a>
							{m.feedEditor.noTagsAfter}
						</p>
					{/if}
				</div>

				<div class="flex flex-col gap-2">
					<label for="channel-q" class="text-sm font-medium">{m.feedEditor.specific}</label>
					<input
						id="channel-q"
						bind:value={channelQuery}
						placeholder={m.feedEditor.searchSubs}
						class="h-10 rounded-card border border-border bg-surface px-3"
					/>
					{#each channelMatches as s (s.channel_id)}
						<div class="flex items-center gap-2 text-sm">
							<span class="flex-1 truncate">{s.name}</span>
							<Button
								size="sm"
								onclick={() => {
									filter.channels = [...new Set([...filter.channels, s.channel_id])];
									channelQuery = '';
								}}>{m.feedEditor.include}</Button
							>
							<Button
								size="sm"
								onclick={() => {
									filter.exclude_channels = [
										...new Set([...filter.exclude_channels, s.channel_id])
									];
									channelQuery = '';
								}}>{m.feedEditor.exclude}</Button
							>
						</div>
					{/each}
					{#each [{ key: 'channels', label: m.feedEditor.included }, { key: 'exclude_channels', label: m.feedEditor.excluded }] as const as group (group.key)}
						{#if filter[group.key].length}
							<div class="flex flex-wrap items-center gap-1.5 text-sm">
								<span class="text-muted">{group.label}:</span>
								{#each filter[group.key] as id (id)}
									<button
										type="button"
										class="rounded-full bg-surface-2 px-2.5 py-1 hover:bg-border"
										onclick={() => (filter[group.key] = filter[group.key].filter((c) => c !== id))}
										aria-label={m.common.removeItem(channelName(id))}>{channelName(id)} ×</button
									>
								{/each}
							</div>
						{/if}
					{/each}
				</div>
			</div>
		</Card>

		<Card title={m.feedEditor.titles}>
			<div class="flex flex-col gap-4">
				<KeywordInput
					label={m.feedEditor.titleAny}
					bind:values={filter.title_any}
					placeholder="rust, linux"
				/>
				<KeywordInput
					label={m.feedEditor.titleNone}
					bind:values={filter.title_none}
					placeholder="#shorts, sponsored"
				/>
				<Field
					label={m.feedEditor.regex}
					bind:value={regex}
					placeholder="\bpart \d+"
					spellcheck="false"
					hint={m.feedEditor.regexHint}
				/>
			</div>
		</Card>

		<Card title={m.feedEditor.videos}>
			<div class="grid gap-4 sm:grid-cols-2">
				<Field
					label={m.feedEditor.minLength}
					type="number"
					min="0"
					step="any"
					bind:value={minMinutes}
				/>
				<Field
					label={m.feedEditor.maxLength}
					type="number"
					min="0"
					step="any"
					bind:value={maxMinutes}
				/>
				<label class="flex flex-col gap-1.5 text-sm font-medium">
					{m.feedEditor.watched}
					<select
						bind:value={filter.watched}
						class="h-11 rounded-card border border-border bg-surface px-3 font-normal"
					>
						<option value="all">{m.feedEditor.showAll}</option>
						<option value="hide_finished">{m.feedEditor.hideWatched}</option>
						<option value="only_unwatched">{m.feedEditor.onlyUnwatched}</option>
						<option value="in_progress">{m.feedEditor.onlyInProgress}</option>
					</select>
				</label>
				<label class="flex flex-col gap-1.5 text-sm font-medium">
					{m.feedEditor.published}
					<select
						value={String(filter.published_within_days ?? '')}
						onchange={(e) =>
							(filter.published_within_days = e.currentTarget.value
								? Number(e.currentTarget.value)
								: null)}
						class="h-11 rounded-card border border-border bg-surface px-3 font-normal"
					>
						<option value="">{m.feedEditor.anyTime}</option>
						<option value="1">{m.feedEditor.lastDay}</option>
						<option value="7">{m.feedEditor.lastWeek}</option>
						<option value="30">{m.feedEditor.lastMonth}</option>
						<option value="365">{m.feedEditor.lastYear}</option>
					</select>
				</label>
				<label class="flex items-center gap-2 text-sm">
					<input
						type="checkbox"
						bind:checked={filter.shorts}
						class="size-4 accent-[var(--accent)]"
					/>
					{m.feedEditor.includeShorts}
				</label>
				<label class="flex items-center gap-2 text-sm">
					<input type="checkbox" bind:checked={filter.live} class="size-4 accent-[var(--accent)]" />
					{m.feedEditor.includeLive}
				</label>
			</div>
		</Card>

		<div class="flex flex-wrap gap-2">
			<Button type="submit" variant="primary" loading={busy}
				>{feed ? m.common.save : m.feedEditor.create}</Button
			>
			<Button onclick={() => history.back()}>{m.common.cancel}</Button>
			{#if feed}
				<Button variant="danger" class="ml-auto" onclick={remove}>{m.feedEditor.delete}</Button>
			{/if}
		</div>
	</form>

	<section aria-label={m.feedEditor.preview} class="flex flex-col gap-4">
		<h2 class="text-lg font-semibold xl:mt-12">{m.feedEditor.preview}</h2>
		{#if previewError}
			<Alert>{previewError}</Alert>
		{:else if preview.length === 0}
			<p class="text-muted">{m.feedEditor.noMatches}</p>
		{/if}
		<div class="grid grid-cols-1 gap-x-4 gap-y-6 sm:grid-cols-2 2xl:grid-cols-3">
			{#each preview.slice(0, 12) as item (item.video.id)}
				<VideoCard video={item.video} progress={item.progress} />
			{/each}
		</div>
	</section>
</div>
