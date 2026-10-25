<script lang="ts">
	import type { HTMLButtonAttributes } from 'svelte/elements';

	interface Props extends HTMLButtonAttributes {
		variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
		size?: 'sm' | 'md';
		loading?: boolean;
		/** Render as a link that looks like a button. */
		href?: string;
	}

	let {
		variant = 'secondary',
		size = 'md',
		loading = false,
		disabled,
		type = 'button',
		class: cls = '',
		href,
		children,
		...rest
	}: Props = $props();

	const variants = {
		primary: 'bg-accent text-accent-fg hover:brightness-110',
		secondary: 'bg-surface-2 text-fg border border-border hover:bg-border/60',
		ghost: 'text-fg hover:bg-surface-2',
		danger: 'bg-danger text-danger-fg hover:brightness-110'
	};
	const sizes = { sm: 'h-8 px-3 text-sm', md: 'h-10 px-4' };
	const classes = $derived(
		`inline-flex items-center justify-center gap-2 rounded-card font-medium transition disabled:cursor-not-allowed disabled:opacity-50 ${variants[variant]} ${sizes[size]} ${cls}`
	);
</script>

{#if href}
	<a {href} class={classes}>{@render children?.()}</a>
{:else}
	<button {type} disabled={disabled || loading} aria-busy={loading} class={classes} {...rest}>
		{#if loading}
			<span
				class="size-4 animate-spin rounded-full border-2 border-current border-t-transparent"
				aria-hidden="true"
			></span>
		{/if}
		{@render children?.()}
	</button>
{/if}
