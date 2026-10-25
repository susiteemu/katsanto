<script lang="ts">
	import { onMount } from 'svelte';
	import {
		CATEGORIES,
		CATEGORY_COLORS,
		communitySettings,
		features,
		saveCommunitySettings,
		type Category,
		type CommunitySettings,
		type SegmentAction
	} from '#lib/community.svelte.js';
	import { m } from '#lib/i18n/index.js';
	import Card from './Card.svelte';

	const s = $derived(communitySettings());
	const f = $derived(features.value);

	onMount(() => void features.load());

	function update(fn: (s: CommunitySettings) => void) {
		const next = $state.snapshot(s) as CommunitySettings;
		fn(next);
		saveCommunitySettings(next);
	}

	const actions = (c: Category): SegmentAction[] =>
		c === 'poi_highlight' ? ['button', 'mark', 'off'] : ['skip', 'button', 'mark', 'off'];
</script>

<Card title={m.community.sponsorblock} description={m.community.sponsorblockHint}>
	{#if f && !f.sponsorblock}
		<p class="text-sm text-muted">{m.community.off}</p>
	{:else}
		<label class="mb-4 flex items-center gap-2">
			<input
				type="checkbox"
				class="size-4 accent-[var(--accent)]"
				checked={s.sponsorblock.enabled}
				onchange={(e) => update((n) => (n.sponsorblock.enabled = e.currentTarget.checked))}
			/>
			{m.community.enabled}
		</label>
		{#if s.sponsorblock.enabled}
			<ul class="divide-y divide-border">
				{#each CATEGORIES as c (c)}
					<li class="flex flex-wrap items-center justify-between gap-2 py-2">
						<span class="flex items-center gap-2 text-sm">
							<span class="size-3 rounded-full" style="background: {CATEGORY_COLORS[c]}"></span>
							{m.community.categories[c]}
						</span>
						<select
							value={s.sponsorblock.categories[c]}
							aria-label={m.community.categories[c]}
							onchange={(e) =>
								update(
									(n) => (n.sponsorblock.categories[c] = e.currentTarget.value as SegmentAction)
								)}
							class="h-9 rounded-card border border-border bg-surface px-2 text-sm"
						>
							{#each actions(c) as a (a)}
								<option value={a}
									>{c === 'poi_highlight' && a === 'button'
										? m.community.highlightButton
										: m.community.actions[a]}</option
								>
							{/each}
						</select>
					</li>
				{/each}
			</ul>
		{/if}
	{/if}
</Card>

<Card title={m.community.dearrow} description={m.community.dearrowHint}>
	{#if f && !f.dearrow_titles}
		<p class="text-sm text-muted">{m.community.off}</p>
	{:else}
		<div class="flex flex-wrap gap-x-6 gap-y-2">
			<label class="flex items-center gap-2">
				<input
					type="checkbox"
					class="size-4 accent-[var(--accent)]"
					checked={s.dearrow.titles}
					onchange={(e) => update((n) => (n.dearrow.titles = e.currentTarget.checked))}
				/>
				{m.community.dearrowTitles}
			</label>
			{#if !f || f.dearrow_thumbnails}
				<label class="flex items-center gap-2">
					<input
						type="checkbox"
						class="size-4 accent-[var(--accent)]"
						checked={s.dearrow.thumbnails}
						onchange={(e) => update((n) => (n.dearrow.thumbnails = e.currentTarget.checked))}
					/>
					{m.community.dearrowThumbnails}
				</label>
			{/if}
		</div>
	{/if}
</Card>
