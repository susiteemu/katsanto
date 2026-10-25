<script lang="ts">
	import { listName, m } from '#lib/i18n/index.js';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import { ago, duration, watchedFraction } from '#lib/format.js';
	import Alert from '#lib/components/Alert.svelte';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';

	type Item = Schemas['ListItem'];
	type Sort = 'manual' | 'added' | 'published' | 'duration';

	const id = $derived(Number(page.params.id));
	let list = $state<Schemas['ListInfo'] | null>(null);
	let items = $state<Item[]>([]);
	let others = $state<Schemas['ListInfo'][]>([]);
	let sort = $state<Sort>('manual');
	let hideFinished = $state(false);
	let error = $state('');
	let editing = $state(false);
	let form = $state({ name: '', description: '', remove_watched: false });
	let dragging = $state<string | null>(null);
	let dropTarget = $state<string | null>(null);

	$effect(() => {
		void load(id);
	});

	async function load(listId: number) {
		try {
			const d = await unwrap(api.GET('/api/v1/lists/{id}', { params: { path: { id: listId } } }));
			list = d.list;
			items = d.items;
			others = (await unwrap(api.GET('/api/v1/lists'))).filter((l) => l.id !== listId);
		} catch (e) {
			error = errorMessage(e);
		}
	}

	const shown = $derived.by(() => {
		let rows = hideFinished ? items.filter((i) => !i.progress?.finished) : items;
		if (sort === 'added') rows = rows.toSorted((a, b) => b.added_at - a.added_at);
		if (sort === 'published')
			rows = rows.toSorted((a, b) => (b.video.published_at ?? 0) - (a.video.published_at ?? 0));
		if (sort === 'duration')
			rows = rows.toSorted((a, b) => (a.video.duration_s ?? 0) - (b.video.duration_s ?? 0));
		return rows;
	});
	// Manual order can only be changed while it's what's shown.
	const canReorder = $derived(sort === 'manual' && !hideFinished);
	const totalLeft = $derived(
		items
			.filter((i) => !i.progress?.finished)
			.reduce((s, i) => s + (i.video.duration_s ?? 0) - (i.progress?.position_s ?? 0), 0)
	);

	/** Move `videoId` to just after `after` (null: top), locally and on the server. */
	async function moveTo(videoId: string, after: string | null) {
		const before = items;
		const item = items.find((i) => i.video.id === videoId)!;
		const rest = items.filter((i) => i.video.id !== videoId);
		const idx = after === null ? 0 : rest.findIndex((i) => i.video.id === after) + 1;
		items = [...rest.slice(0, idx), item, ...rest.slice(idx)];
		try {
			await unwrap(
				api.PUT('/api/v1/lists/{id}/items/{video_id}/position', {
					params: { path: { id, video_id: videoId } },
					body: { after }
				})
			);
		} catch (e) {
			items = before;
			error = errorMessage(e);
		}
	}

	function step(index: number, delta: number) {
		const target = index + delta;
		if (target < 0 || target >= items.length) return;
		const videoId = items[index].video.id;
		// After the item that will precede it.
		const rest = items.filter((i) => i.video.id !== videoId);
		void moveTo(videoId, target === 0 ? null : rest[target - 1].video.id);
	}

	function onDrop(targetId: string) {
		const src = dragging;
		dragging = dropTarget = null;
		if (!src || src === targetId) return;
		// Dropping on an item places the dragged one before it.
		const rest = items.filter((i) => i.video.id !== src);
		const idx = rest.findIndex((i) => i.video.id === targetId);
		void moveTo(src, idx === 0 ? null : rest[idx - 1].video.id);
	}

	async function remove(item: Item) {
		items = items.filter((i) => i !== item);
		await api.DELETE('/api/v1/lists/{id}/items/{video_id}', {
			params: { path: { id, video_id: item.video.id } }
		});
	}

	async function moveToList(item: Item, target: number) {
		try {
			await unwrap(
				api.POST('/api/v1/lists/{id}/items', {
					params: { path: { id: target } },
					body: { video_id: item.video.id }
				})
			);
			await remove(item);
		} catch (e) {
			error = errorMessage(e);
		}
	}

	function openEdit() {
		form = {
			name: list!.name,
			description: list!.description,
			remove_watched: list!.remove_watched
		};
		editing = true;
	}

	async function save(e: SubmitEvent) {
		e.preventDefault();
		try {
			list = await unwrap(api.PUT('/api/v1/lists/{id}', { params: { path: { id } }, body: form }));
			editing = false;
		} catch (err) {
			error = errorMessage(err);
		}
	}

	async function deleteList() {
		if (!confirm(m.lists.confirmDelete(list?.name ?? ''))) return;
		await unwrap(api.DELETE('/api/v1/lists/{id}', { params: { path: { id } } }));
		await goto('/lists');
	}

	function play(shuffle = false) {
		const playable = shown.filter((i) => !i.progress?.finished);
		const first = (playable.length ? playable : shown)[0];
		if (!first) return;
		void goto(`/watch/${first.video.id}?list=${id}${shuffle ? '&shuffle=1' : ''}`);
	}
</script>

<svelte:head><title>{list ? `${listName(list)} · Katsanto` : m.lists.listTitle}</title></svelte:head
>

<div class="mx-auto flex max-w-4xl flex-col gap-5">
	{#if error}<Alert>{error}</Alert>{/if}
	{#if list}
		<header class="flex flex-col gap-3">
			<div class="flex flex-wrap items-start justify-between gap-3">
				<div class="min-w-0">
					<h1 class="text-2xl font-semibold">{listName(list)}</h1>
					{#if list.description}<p class="text-muted">{list.description}</p>{/if}
					<p class="mt-1 text-sm text-muted">
						{m.common.videos(items.length)}{totalLeft > 0
							? ` · ${m.lists.left(duration(totalLeft))}`
							: ''}
						{#if list.remove_watched}· {m.lists.removesWatched}{/if}
					</p>
				</div>
				<div class="flex flex-wrap gap-2">
					<Button variant="primary" onclick={() => play()} disabled={items.length === 0}
						><svg viewBox="0 0 24 24" class="size-4" fill="currentColor" aria-hidden="true"
							><path d="M8 5v14l11-7z" /></svg
						>
						{m.lists.playAll}</Button
					>
					<Button onclick={() => play(true)} disabled={items.length < 2}>{m.lists.shuffle}</Button>
					<Button variant="ghost" onclick={openEdit}>{m.common.edit}</Button>
					{#if list.kind === 'custom'}
						<Button variant="ghost" onclick={deleteList}>{m.common.delete}</Button>
					{/if}
				</div>
			</div>
			<div class="flex flex-wrap items-center gap-3 text-sm">
				<select
					bind:value={sort}
					aria-label={m.lists.sort}
					class="h-8 rounded-full border border-border bg-surface px-3"
				>
					<option value="manual">{m.lists.sortManual}</option>
					<option value="added">{m.lists.sortAdded}</option>
					<option value="published">{m.lists.sortPublished}</option>
					<option value="duration">{m.lists.sortDuration}</option>
				</select>
				<label class="flex items-center gap-2">
					<input
						type="checkbox"
						bind:checked={hideFinished}
						class="size-4 accent-[var(--accent)]"
					/>
					{m.lists.hideWatched}
				</label>
			</div>
		</header>

		{#if items.length === 0}
			<div class="rounded-card border border-dashed border-border p-10 text-center text-muted">
				{m.lists.empty}
			</div>
		{/if}

		<ol class="flex flex-col gap-1">
			{#each shown as item, i (item.video.id)}
				{@const w = watchedFraction(item.progress)}
				<li
					class="group flex items-center gap-3 rounded-card p-2 transition hover:bg-surface-2 {dropTarget ===
					item.video.id
						? 'ring-2 ring-accent'
						: ''} {dragging === item.video.id ? 'opacity-40' : ''}"
					draggable={canReorder}
					ondragstart={(e) => {
						dragging = item.video.id;
						e.dataTransfer?.setData('text/plain', item.video.id);
					}}
					ondragend={() => (dragging = dropTarget = null)}
					ondragover={(e) => {
						if (!dragging) return;
						e.preventDefault();
						dropTarget = item.video.id;
					}}
					ondragleave={() => dropTarget === item.video.id && (dropTarget = null)}
					ondrop={(e) => {
						e.preventDefault();
						onDrop(item.video.id);
					}}
				>
					{#if canReorder}
						<span class="hidden w-5 cursor-grab text-center text-muted md:block" aria-hidden="true"
							>⋮⋮</span
						>
					{/if}
					<a
						href="/watch/{item.video.id}?list={id}"
						class="relative block w-36 shrink-0 overflow-hidden rounded-card bg-surface-2 sm:w-44"
					>
						<div class="aspect-video">
							{#if item.video.thumbnail_url}
								<img
									src={item.video.thumbnail_url}
									alt=""
									loading="lazy"
									class="size-full object-cover"
								/>
							{/if}
						</div>
						{#if item.video.duration_s}
							<span
								class="absolute right-1 bottom-1 rounded bg-black/80 px-1 text-xs text-white tabular-nums"
								>{duration(item.video.duration_s)}</span
							>
						{/if}
						{#if w !== null}
							<div class="absolute inset-x-0 bottom-0 h-1 bg-white/30">
								<div class="h-full bg-accent" style="width: {w * 100}%"></div>
							</div>
						{/if}
					</a>
					<div class="min-w-0 flex-1">
						<a
							href="/watch/{item.video.id}?list={id}"
							class="line-clamp-2 font-medium hover:underline">{item.video.title}</a
						>
						<p class="truncate text-sm text-muted">
							{item.video.channel?.name} · {m.lists.added(ago(item.added_at))}
						</p>
					</div>
					<div class="flex shrink-0 items-center">
						{#if canReorder}
							<button
								class="grid size-8 place-items-center rounded-full text-muted hover:bg-border hover:text-fg disabled:opacity-30"
								disabled={i === 0}
								onclick={() => step(i, -1)}
								aria-label={m.lists.moveUp(item.video.title)}>↑</button
							>
							<button
								class="grid size-8 place-items-center rounded-full text-muted hover:bg-border hover:text-fg disabled:opacity-30"
								disabled={i === shown.length - 1}
								onclick={() => step(i, 1)}
								aria-label={m.lists.moveDown(item.video.title)}>↓</button
							>
						{/if}
						{#if others.length}
							<select
								class="h-8 w-8 cursor-pointer appearance-none rounded-full bg-transparent text-center text-muted hover:bg-border"
								aria-label={m.lists.moveToOther(item.video.title)}
								title={m.lists.moveToOtherTitle}
								value=""
								onchange={(e) => moveToList(item, Number(e.currentTarget.value))}
							>
								<option value="" disabled>⇢</option>
								{#each others as o (o.id)}<option value={o.id}>{m.lists.moveTo(listName(o))}</option
									>{/each}
							</select>
						{/if}
						<button
							class="grid size-8 place-items-center rounded-full text-muted hover:bg-border hover:text-fg"
							onclick={() => remove(item)}
							aria-label={m.lists.removeFrom(item.video.title)}
							title={m.common.remove}><Icon name="trash" size={16} /></button
						>
					</div>
				</li>
			{/each}
		</ol>
	{/if}
</div>

<Dialog bind:open={editing} title={m.lists.editList}>
	<form class="flex flex-col gap-4" onsubmit={save}>
		<Field label={m.lists.name} bind:value={form.name} required maxlength={60} />
		<Field label={m.lists.description} bind:value={form.description} maxlength={500} />
		<label class="flex items-center gap-2 text-sm">
			<input
				type="checkbox"
				bind:checked={form.remove_watched}
				class="size-4 accent-[var(--accent)]"
			/>
			{m.lists.removeWatched}
		</label>
		<div class="flex gap-2">
			<Button type="submit" variant="primary">{m.common.save}</Button>
			<Button onclick={() => (editing = false)}>{m.common.cancel}</Button>
		</div>
	</form>
</Dialog>
