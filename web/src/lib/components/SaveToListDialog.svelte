<script lang="ts">
	import { listName, m } from '#lib/i18n/index.js';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import { saveToList } from '#lib/saveToList.svelte.js';
	import Alert from './Alert.svelte';
	import Button from './Button.svelte';
	import Dialog from './Dialog.svelte';

	let lists = $state<Schemas['ListInfo'][] | null>(null);
	let newName = $state('');
	let error = $state('');
	let busy = $state(false);
	const open = $derived(saveToList.video !== null);

	$effect(() => {
		const v = saveToList.video;
		lists = null;
		error = '';
		if (v) void load(v.id);
	});

	async function load(videoId: string) {
		try {
			lists = await unwrap(api.GET('/api/v1/lists', { params: { query: { contains: videoId } } }));
		} catch (e) {
			error = errorMessage(e);
		}
	}

	async function toggle(list: Schemas['ListInfo']) {
		const video_id = saveToList.video!.id;
		busy = true;
		error = '';
		try {
			if (list.contains) {
				await unwrap(
					api.DELETE('/api/v1/lists/{id}/items/{video_id}', {
						params: { path: { id: list.id, video_id } }
					})
				);
			} else {
				await unwrap(
					api.POST('/api/v1/lists/{id}/items', {
						params: { path: { id: list.id } },
						body: { video_id }
					})
				);
			}
			list.contains = !list.contains;
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}

	async function create(e: SubmitEvent) {
		e.preventDefault();
		const name = newName.trim();
		if (!name) return;
		busy = true;
		error = '';
		try {
			const list = await unwrap(api.POST('/api/v1/lists', { body: { name } }));
			await unwrap(
				api.POST('/api/v1/lists/{id}/items', {
					params: { path: { id: list.id } },
					body: { video_id: saveToList.video!.id }
				})
			);
			lists = [...(lists ?? []), { ...list, contains: true }];
			newName = '';
		} catch (err) {
			error = errorMessage(err);
		} finally {
			busy = false;
		}
	}
</script>

<Dialog {open} title={m.lists.saveTo} onclose={() => saveToList.close()}>
	<div class="flex flex-col gap-4">
		{#if saveToList.video}
			<p class="line-clamp-2 text-sm text-muted">{saveToList.video.title}</p>
		{/if}
		{#if error}<Alert>{error}</Alert>{/if}
		{#if lists}
			<ul class="flex max-h-72 flex-col overflow-y-auto">
				{#each lists as l (l.id)}
					<li>
						<label
							class="flex cursor-pointer items-center gap-3 rounded-card px-2 py-2 hover:bg-surface-2"
						>
							<input
								type="checkbox"
								checked={l.contains ?? false}
								disabled={busy}
								onchange={() => toggle(l)}
								class="size-4 accent-[var(--accent)]"
							/>
							<span class="flex-1 truncate">{listName(l)}</span>
							<span class="text-xs text-muted">{l.item_count}</span>
						</label>
					</li>
				{/each}
			</ul>
		{:else if !error}
			<p class="text-muted">{m.common.loading}</p>
		{/if}
		<form class="flex gap-2" onsubmit={create}>
			<input
				bind:value={newName}
				placeholder={m.lists.newList}
				aria-label={m.lists.newListName}
				maxlength="60"
				class="h-10 min-w-0 flex-1 rounded-card border border-border bg-surface px-3"
			/>
			<Button type="submit" disabled={!newName.trim()} loading={busy}>{m.common.create}</Button>
		</form>
		<Button variant="ghost" onclick={() => saveToList.close()}>{m.lists.done}</Button>
	</div>
</Dialog>
