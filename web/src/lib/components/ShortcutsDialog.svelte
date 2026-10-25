<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import Dialog from './Dialog.svelte';

	let open = $state(false);

	const GROUPS = $derived.by(() => {
		const k = m.shortcuts;
		const groups: { title: string; keys: [string[], string][] }[] = [
			{
				title: k.general,
				keys: [
					[['/'], k.search],
					[['?'], k.help]
				]
			},
			{
				title: k.player,
				keys: [
					[['Space', 'K'], k.playPause],
					[['J', 'L'], k.seek10],
					[['←', '→'], k.seek5],
					[['0–9'], k.percent],
					[['<', '>'], k.speed],
					[['M'], k.mute],
					[['C'], k.captions],
					[['F'], k.fullscreen],
					[['I'], k.pip],
					[['↑', '↓'], k.volume]
				]
			},
			{
				title: k.list,
				keys: [
					[['Shift', 'N'], k.next],
					[['Shift', 'P'], k.previous]
				]
			}
		];
		return groups;
	});

	function onkeydown(e: KeyboardEvent) {
		const t = e.target as HTMLElement;
		if (e.key !== '?' || e.ctrlKey || e.metaKey || e.altKey) return;
		if (/INPUT|TEXTAREA|SELECT/.test(t.tagName) || t.isContentEditable) return;
		e.preventDefault();
		open = !open;
	}
</script>

<svelte:window {onkeydown} />

<Dialog bind:open title={m.shortcuts.title}>
	<div class="flex max-h-[70dvh] flex-col gap-5 overflow-y-auto">
		{#each GROUPS as g (g.title)}
			<section>
				<h3 class="mb-2 text-sm font-medium text-muted">{g.title}</h3>
				<dl class="grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-2 text-sm">
					{#each g.keys as [keys, label] (label)}
						<dt class="flex gap-1">
							{#each keys as k (k)}
								<kbd
									class="min-w-7 rounded border border-border bg-surface-2 px-1.5 py-0.5 text-center font-mono text-xs"
									>{k}</kbd
								>
							{/each}
						</dt>
						<dd>{label}</dd>
					{/each}
				</dl>
			</section>
		{/each}
	</div>
	<form method="dialog" class="mt-5 flex justify-end">
		<button class="rounded-full bg-surface-2 px-4 py-1.5 text-sm hover:bg-border"
			>{m.common.close}</button
		>
	</form>
</Dialog>
