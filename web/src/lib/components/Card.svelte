<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		title,
		description,
		actions,
		children
	}: { title?: string; description?: string; actions?: Snippet; children: Snippet } = $props();
</script>

<section class="rounded-card border border-border bg-surface p-5 shadow-card">
	{#if title || actions}
		<header class="mb-4 flex flex-wrap items-start justify-between gap-3">
			<div>
				{#if title}<h2 class="text-lg font-semibold">{title}</h2>{/if}
				{#if description}<p class="mt-0.5 text-sm text-muted">{description}</p>{/if}
			</div>
			{@render actions?.()}
		</header>
	{/if}
	{@render children()}
</section>
