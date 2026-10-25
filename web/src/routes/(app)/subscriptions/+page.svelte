<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { onMount } from 'svelte';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import { ago, compact } from '#lib/format.js';
	import { subscriptions } from '#lib/subscriptions.svelte.js';
	import Alert from '#lib/components/Alert.svelte';
	import Button from '#lib/components/Button.svelte';
	import TagChip from '#lib/components/TagChip.svelte';

	type Sub = Schemas['SubscriptionInfo'];

	let list = $state<Sub[] | null>(null);
	let tags = $state<Schemas['TagInfo'][]>([]);
	/** `all`, `untagged` or a tag id. */
	let tagFilter = $state('all');
	let bulkTag = $state('');
	/** Channel whose tag editor is open. */
	let editing = $state<string | null>(null);
	let newTag = $state('');
	let filter = $state('');
	let sort = $state<'name' | 'recent'>('name');
	let selected = $state(new Set<string>());
	let busy = $state(false);
	let error = $state('');

	onMount(load);

	async function load() {
		try {
			[list, tags] = await Promise.all([
				unwrap(api.GET('/api/v1/subscriptions')),
				unwrap(api.GET('/api/v1/tags'))
			]);
		} catch (e) {
			error = errorMessage(e);
		}
	}

	const shown = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		const rows = (list ?? []).filter(
			(s) =>
				(!q || s.name.toLowerCase().includes(q) || s.handle?.toLowerCase().includes(q)) &&
				(tagFilter === 'all' ||
					(tagFilter === 'untagged'
						? s.tag_ids.length === 0
						: s.tag_ids.includes(Number(tagFilter))))
		);
		return sort === 'recent'
			? rows.toSorted((a, b) => (b.last_video_at ?? 0) - (a.last_video_at ?? 0))
			: rows;
	});
	const allShownSelected = $derived(
		shown.length > 0 && shown.every((s) => selected.has(s.channel_id))
	);

	function toggle(id: string) {
		const next = new Set(selected);
		if (next.has(id)) next.delete(id);
		else next.add(id);
		selected = next;
	}

	function toggleAll() {
		selected = allShownSelected ? new Set() : new Set(shown.map((s) => s.channel_id));
	}

	async function unsubscribe(ids: string[]) {
		const names = (list ?? []).filter((s) => ids.includes(s.channel_id)).map((s) => s.name);
		const what = names.length === 1 ? names[0] : m.subs.channels(names.length);
		if (!confirm(m.channel.confirmUnsubscribe(what))) return;
		busy = true;
		error = '';
		try {
			await subscriptions.unsubscribe(ids);
			list = (list ?? []).filter((s) => !ids.includes(s.channel_id));
			selected = new Set();
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}

	const tag = (id: number) => tags.find((t) => t.id === id);

	/** Create a tag (from the inline editor) and return it. */
	async function createTag(name: string) {
		const t = await unwrap(api.POST('/api/v1/tags', { body: { name, color: '' } }));
		tags = [...tags, t].toSorted((a, b) => a.name.localeCompare(b.name));
		return t;
	}

	async function setTags(s: Sub, tagIds: number[]) {
		try {
			await unwrap(
				api.PUT('/api/v1/subscriptions/{channel_id}/tags', {
					params: { path: { channel_id: s.channel_id } },
					body: { tag_ids: tagIds }
				})
			);
			s.tag_ids = tagIds;
		} catch (e) {
			error = errorMessage(e);
		}
	}

	async function addNewTag(s: Sub) {
		const name = newTag.trim();
		if (!name) return;
		try {
			const t =
				tags.find((x) => x.name.toLowerCase() === name.toLowerCase()) ?? (await createTag(name));
			await setTags(s, [...new Set([...s.tag_ids, t.id])]);
			newTag = '';
		} catch (e) {
			error = errorMessage(e);
		}
	}

	async function bulkTagging(add: boolean) {
		const id = Number(bulkTag);
		if (!id) return;
		const ids = [...selected];
		busy = true;
		try {
			await unwrap(
				api.POST('/api/v1/tags/{id}/channels', {
					params: { path: { id } },
					body: { channel_ids: ids, add }
				})
			);
			for (const s of list ?? []) {
				if (!ids.includes(s.channel_id)) continue;
				s.tag_ids = add ? [...new Set([...s.tag_ids, id])] : s.tag_ids.filter((t) => t !== id);
			}
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}

	function status(s: Sub) {
		if (s.fetch_error) return { text: m.subs.refreshFailing, cls: 'text-danger' };
		if (!s.fetched_at) return { text: m.subs.loadingVideos, cls: 'text-muted' };
		return {
			text: s.last_video_at ? m.subs.latestVideo(ago(s.last_video_at)) : m.subs.noVideos,
			cls: 'text-muted'
		};
	}
</script>

<svelte:head><title>{m.subs.title}</title></svelte:head>

<div class="mx-auto flex max-w-4xl flex-col gap-5">
	<div class="flex flex-wrap items-end justify-between gap-3">
		<div>
			<h1 class="text-2xl font-semibold">{m.subs.heading}</h1>
			{#if list}<p class="text-sm text-muted">{m.subs.channels(list.length)}</p>{/if}
		</div>
		<div class="flex flex-wrap gap-2">
			<input
				type="search"
				bind:value={filter}
				placeholder={m.subs.filter}
				aria-label={m.subs.filterLabel}
				class="h-9 w-48 rounded-full border border-border bg-surface px-4 text-sm"
			/>
			<select
				bind:value={tagFilter}
				aria-label={m.subs.filterByTag}
				class="h-9 rounded-full border border-border bg-surface px-3 text-sm"
			>
				<option value="all">{m.subs.allTags}</option>
				<option value="untagged">{m.subs.untagged}</option>
				{#each tags as t (t.id)}<option value={String(t.id)}>{t.name}</option>{/each}
			</select>
			<select
				bind:value={sort}
				aria-label={m.subs.sortBy}
				class="h-9 rounded-full border border-border bg-surface px-3 text-sm"
			>
				<option value="name">{m.subs.sortName}</option>
				<option value="recent">{m.subs.sortRecent}</option>
			</select>
		</div>
	</div>

	{#if error}<Alert>{error}</Alert>{/if}

	{#if list && list.length === 0}
		<div class="rounded-card border border-dashed border-border p-10 text-center">
			<p class="mb-3 text-muted">{m.subs.empty}</p>
			<a href="/search" class="text-accent underline">{m.subs.findChannels}</a>
		</div>
	{:else if list}
		<div class="flex items-center gap-3 border-b border-border pb-3 text-sm">
			<label class="flex items-center gap-2">
				<input
					type="checkbox"
					checked={allShownSelected}
					onchange={toggleAll}
					class="size-4 accent-[var(--accent)]"
				/>
				{m.subs.selectAll}
			</label>
			{#if selected.size && tags.length}
				<select
					bind:value={bulkTag}
					aria-label={m.subs.bulkTag}
					class="h-8 rounded-full border border-border bg-surface px-3 text-sm"
				>
					<option value="">{m.subs.tagPlaceholder}</option>
					{#each tags as t (t.id)}<option value={String(t.id)}>{t.name}</option>{/each}
				</select>
				<Button size="sm" disabled={!bulkTag} loading={busy} onclick={() => bulkTagging(true)}
					>{m.subs.addTag}</Button
				>
				<Button size="sm" disabled={!bulkTag} loading={busy} onclick={() => bulkTagging(false)}
					>{m.subs.removeTag}</Button
				>
			{/if}
			{#if selected.size}
				<Button
					size="sm"
					variant="danger"
					loading={busy}
					onclick={() => unsubscribe([...selected])}
				>
					{m.subs.unsubscribeN(selected.size)}
				</Button>
			{/if}
		</div>

		<ul class="divide-y divide-border">
			{#each shown as s (s.channel_id)}
				{@const st = status(s)}
				<li class="flex items-center gap-3 py-3">
					<input
						type="checkbox"
						checked={selected.has(s.channel_id)}
						onchange={() => toggle(s.channel_id)}
						aria-label={m.subs.select(s.name)}
						class="size-4 shrink-0 accent-[var(--accent)]"
					/>
					<a href="/channel/{s.channel_id}" class="flex min-w-0 flex-1 items-center gap-3">
						<img
							src={s.avatar_url}
							alt=""
							class="size-11 shrink-0 rounded-full bg-surface-2"
							loading="lazy"
						/>
						<div class="min-w-0">
							<p class="truncate font-medium">{s.name}</p>
							<p class="truncate text-sm">
								<span class={st.cls} title={s.fetch_error ?? undefined}>{st.text}</span>
								{#if s.subscriber_count != null}
									<span class="text-muted">
										· {m.channel.subscribers(compact(s.subscriber_count))}</span
									>
								{/if}
							</p>
							{#if s.tag_ids.length}
								<div class="mt-1 flex flex-wrap gap-1">
									{#each s.tag_ids as id (id)}
										{@const t = tag(id)}
										{#if t}<TagChip name={t.name} color={t.color} />{/if}
									{/each}
								</div>
							{/if}
						</div>
					</a>
					<Button
						size="sm"
						variant="ghost"
						aria-expanded={editing === s.channel_id}
						onclick={() => (editing = editing === s.channel_id ? null : s.channel_id)}
						>{m.subs.tags}</Button
					>
					<Button size="sm" variant="ghost" onclick={() => unsubscribe([s.channel_id])}
						>{m.subs.unsubscribe}</Button
					>
				</li>
				{#if editing === s.channel_id}
					<li class="flex flex-wrap items-center gap-1.5 bg-surface-2/50 px-3 py-3">
						{#each tags as t (t.id)}
							<TagChip
								name={t.name}
								color={t.color}
								selected={s.tag_ids.includes(t.id)}
								onclick={() =>
									setTags(
										s,
										s.tag_ids.includes(t.id)
											? s.tag_ids.filter((x) => x !== t.id)
											: [...s.tag_ids, t.id]
									)}
							/>
						{/each}
						<form
							class="flex items-center gap-1.5"
							onsubmit={(e) => {
								e.preventDefault();
								void addNewTag(s);
							}}
						>
							<input
								bind:value={newTag}
								placeholder={m.subs.newTag}
								aria-label={m.subs.newTagFor(s.name)}
								maxlength="30"
								class="h-7 w-32 rounded-full border border-border bg-surface px-3 text-xs"
							/>
							<Button size="sm" type="submit" disabled={!newTag.trim()}>{m.subs.add}</Button>
						</form>
					</li>
				{/if}
			{/each}
		</ul>
	{:else if !error}
		<p class="text-muted">{m.common.loading}</p>
	{/if}
</div>
