<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { onMount } from 'svelte';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import { TAG_COLORS } from '#lib/tags.js';
	import Alert from '#lib/components/Alert.svelte';
	import Button from '#lib/components/Button.svelte';
	import Card from '#lib/components/Card.svelte';
	import TagChip from '#lib/components/TagChip.svelte';

	let feeds = $state<Schemas['FeedInfo'][]>([]);
	let tags = $state<Schemas['TagInfo'][]>([]);
	let error = $state('');

	onMount(load);

	async function load() {
		try {
			[feeds, tags] = await Promise.all([
				unwrap(api.GET('/api/v1/feeds')),
				unwrap(api.GET('/api/v1/tags'))
			]);
		} catch (e) {
			error = errorMessage(e);
		}
	}

	async function move(i: number, delta: number) {
		const next = [...feeds];
		const [f] = next.splice(i, 1);
		next.splice(i + delta, 0, f);
		feeds = next;
		await api.PUT('/api/v1/feeds/order', { body: { ids: next.map((f) => f.id) } });
	}

	async function removeFeed(f: Schemas['FeedInfo']) {
		if (!confirm(m.feeds.confirmDeleteFeed(f.name))) return;
		await api.DELETE('/api/v1/feeds/{id}', { params: { path: { id: f.id } } });
		feeds = feeds.filter((x) => x.id !== f.id);
	}

	async function editTag(t: Schemas['TagInfo']) {
		const name = prompt(m.feeds.tagName, t.name)?.trim();
		if (!name) return;
		try {
			await unwrap(
				api.PUT('/api/v1/tags/{id}', {
					params: { path: { id: t.id } },
					body: { name, color: t.color }
				})
			);
			await load();
		} catch (e) {
			error = errorMessage(e);
		}
	}

	async function recolor(t: Schemas['TagInfo'], color: string) {
		await unwrap(
			api.PUT('/api/v1/tags/{id}', {
				params: { path: { id: t.id } },
				body: { name: t.name, color }
			})
		);
		await load();
	}

	async function removeTag(t: Schemas['TagInfo']) {
		if (!confirm(m.feeds.confirmDeleteTag(t.name))) return;
		await api.DELETE('/api/v1/tags/{id}', { params: { path: { id: t.id } } });
		tags = tags.filter((x) => x.id !== t.id);
	}
</script>

<svelte:head><title>{m.feeds.title}</title></svelte:head>

<div class="mx-auto flex max-w-3xl flex-col gap-6">
	<h1 class="text-2xl font-semibold">{m.feeds.heading}</h1>
	{#if error}<Alert>{error}</Alert>{/if}

	<Card title={m.feeds.feeds} description={m.feeds.feedsHint}>
		{#snippet actions()}
			<Button href="/feeds/new" size="sm" variant="primary">{m.feeds.newFeed}</Button>
		{/snippet}
		{#if feeds.length === 0}
			<p class="text-sm text-muted">{m.feeds.noFeeds}</p>
		{/if}
		<ul class="divide-y divide-border">
			{#each feeds as f, i (f.id)}
				<li class="flex items-center gap-2 py-2.5">
					<span class="w-6 text-center">{f.icon}</span>
					<a href="/?feed={f.id}" class="flex-1 truncate font-medium hover:underline">{f.name}</a>
					<Button
						size="sm"
						variant="ghost"
						disabled={i === 0}
						onclick={() => move(i, -1)}
						aria-label={m.feeds.moveUp(f.name)}>↑</Button
					>
					<Button
						size="sm"
						variant="ghost"
						disabled={i === feeds.length - 1}
						onclick={() => move(i, 1)}
						aria-label={m.feeds.moveDown(f.name)}>↓</Button
					>
					<Button href="/feeds/{f.id}" size="sm" variant="ghost">{m.common.edit}</Button>
					<Button size="sm" variant="ghost" onclick={() => removeFeed(f)}>{m.common.delete}</Button>
				</li>
			{/each}
		</ul>
	</Card>

	<Card title={m.feeds.tags} description={m.feeds.tagsHint}>
		{#if tags.length === 0}
			<p class="text-sm text-muted">{m.feeds.noTags}</p>
		{/if}
		<ul class="divide-y divide-border">
			{#each tags as t (t.id)}
				<li class="flex flex-wrap items-center gap-2 py-2.5">
					<span class="flex-1"><TagChip name={t.name} color={t.color} /></span>
					<select
						value={t.color}
						onchange={(e) => recolor(t, e.currentTarget.value)}
						aria-label={m.feeds.colorOf(t.name)}
						class="h-8 rounded-full border border-border bg-surface px-2 text-sm"
					>
						{#each TAG_COLORS as c (c.id + c.label)}<option value={c.id}
								>{m.feeds.colors[c.id]}</option
							>{/each}
					</select>
					<Button size="sm" variant="ghost" onclick={() => editTag(t)}>{m.feeds.rename}</Button>
					<Button size="sm" variant="ghost" onclick={() => removeTag(t)}>{m.common.delete}</Button>
				</li>
			{/each}
		</ul>
	</Card>
</div>
