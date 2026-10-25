<script lang="ts">
	import type { Snippet } from 'svelte';
	import { tagColor } from '#lib/tags.js';

	let {
		name,
		color = '',
		selected = false,
		onclick,
		children
	}: {
		name: string;
		color?: string;
		/** For toggle chips. */
		selected?: boolean;
		onclick?: () => void;
		children?: Snippet;
	} = $props();
	const c = $derived(tagColor(color));
</script>

{#if onclick}
	<button
		type="button"
		{onclick}
		aria-pressed={selected}
		class="inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-xs font-medium transition"
		style="border-color: {selected ? c : 'var(--border)'}; background: {selected
			? `color-mix(in oklch, ${c} 18%, transparent)`
			: 'transparent'}"
	>
		<span class="size-2 rounded-full" style="background: {c}"></span>{name}{@render children?.()}
	</button>
{:else}
	<span
		class="inline-flex h-6 items-center gap-1.5 rounded-full px-2 text-xs font-medium"
		style="background: color-mix(in oklch, {c} 16%, transparent)"
	>
		<span class="size-1.5 rounded-full" style="background: {c}"></span>{name}
	</span>
{/if}
