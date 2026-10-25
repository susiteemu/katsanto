<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';

	interface Props extends HTMLInputAttributes {
		label: string;
		hint?: string;
		value?: string;
	}

	let { label, hint, value = $bindable(''), id, ...rest }: Props = $props();
	const uid = $props.id();
	const inputId = $derived(id ?? `field-${uid}`);
</script>

<div class="flex flex-col gap-1.5">
	<label for={inputId} class="text-sm font-medium">{label}</label>
	<input
		id={inputId}
		bind:value
		class="h-11 rounded-card border border-border bg-surface px-3 text-fg placeholder:text-muted focus:border-accent focus:outline-none"
		aria-describedby={hint ? `${inputId}-hint` : undefined}
		{...rest}
	/>
	{#if hint}
		<p id="{inputId}-hint" class="text-sm text-muted">{hint}</p>
	{/if}
</div>
