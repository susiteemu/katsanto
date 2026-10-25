<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	/** Edit a list of keywords: type and press Enter or comma; click a chip to remove it. */
	let {
		label,
		values = $bindable([]),
		placeholder = ''
	}: { label: string; values?: string[]; placeholder?: string } = $props();
	let text = $state('');
	const uid = $props.id();

	function add() {
		const parts = text
			.split(',')
			.map((s) => s.trim())
			.filter(Boolean);
		if (parts.length) values = [...new Set([...values, ...parts])];
		text = '';
	}

	function onkeydown(e: KeyboardEvent) {
		if (e.key === 'Enter' || e.key === ',') {
			e.preventDefault();
			add();
		} else if (e.key === 'Backspace' && !text && values.length) {
			values = values.slice(0, -1);
		}
	}
</script>

<div class="flex flex-col gap-1.5">
	<label for="kw-{uid}" class="text-sm font-medium">{label}</label>
	<div
		class="flex min-h-11 flex-wrap items-center gap-1.5 rounded-card border border-border bg-surface px-2 py-1.5 focus-within:border-accent"
	>
		{#each values as v (v)}
			<button
				type="button"
				class="inline-flex h-7 items-center gap-1 rounded-full bg-surface-2 px-2.5 text-sm hover:bg-border"
				onclick={() => (values = values.filter((x) => x !== v))}
				aria-label={m.common.removeItem(v)}
				>{v} <span aria-hidden="true" class="text-muted">×</span></button
			>
		{/each}
		<input
			id="kw-{uid}"
			bind:value={text}
			{onkeydown}
			onblur={add}
			{placeholder}
			class="h-7 min-w-24 flex-1 bg-transparent px-1 outline-none"
		/>
	</div>
</div>
