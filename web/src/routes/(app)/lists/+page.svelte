<script lang="ts">
	import { listName, m } from '#lib/i18n/index.js';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import { duration } from '#lib/format.js';
	import Alert from '#lib/components/Alert.svelte';
	import Button from '#lib/components/Button.svelte';
	import Icon from '#lib/components/Icon.svelte';

	let lists = $state<Schemas['ListInfo'][] | null>(null);
	let name = $state('');
	let error = $state('');

	onMount(async () => {
		try {
			lists = await unwrap(api.GET('/api/v1/lists'));
		} catch (e) {
			error = errorMessage(e);
		}
	});

	async function create(e: SubmitEvent) {
		e.preventDefault();
		try {
			const l = await unwrap(api.POST('/api/v1/lists', { body: { name } }));
			await goto(`/lists/${l.id}`);
		} catch (err) {
			error = errorMessage(err);
		}
	}
</script>

<svelte:head><title>{m.lists.title}</title></svelte:head>

<div class="mx-auto flex max-w-[110rem] flex-col gap-6">
	<div class="flex flex-wrap items-end justify-between gap-3">
		<h1 class="text-2xl font-semibold">{m.lists.heading}</h1>
		<form class="flex gap-2" onsubmit={create}>
			<input
				bind:value={name}
				placeholder={m.lists.newList}
				aria-label={m.lists.newListName}
				maxlength="60"
				required
				class="h-9 w-56 rounded-full border border-border bg-surface px-4 text-sm"
			/>
			<Button size="sm" type="submit" variant="primary"
				><Icon name="plus" size={16} /> {m.common.create}</Button
			>
		</form>
	</div>
	{#if error}<Alert>{error}</Alert>{/if}

	<div class="grid grid-cols-1 gap-x-4 gap-y-8 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4">
		{#each lists ?? [] as l (l.id)}
			<a href="/lists/{l.id}" class="group flex flex-col gap-2">
				<div class="relative aspect-video overflow-hidden rounded-card bg-surface-2">
					{#if l.thumbnail_url}
						<img
							src={l.thumbnail_url}
							alt=""
							loading="lazy"
							class="size-full object-cover transition duration-300 group-hover:scale-[1.03]"
						/>
					{:else}
						<div class="grid size-full place-items-center text-muted">
							<Icon name="list" size={32} />
						</div>
					{/if}
					<span
						class="absolute right-0 bottom-0 left-0 flex justify-between bg-black/70 px-3 py-1.5 text-xs font-medium text-white"
					>
						<span>{m.common.videos(l.item_count)}</span>
						{#if l.total_duration_s}<span>{duration(l.total_duration_s)}</span>{/if}
					</span>
				</div>
				<div>
					<h2 class="font-medium group-hover:underline">{listName(l)}</h2>
					{#if l.description}<p class="line-clamp-1 text-sm text-muted">{l.description}</p>{/if}
				</div>
			</a>
		{/each}
	</div>
</div>
