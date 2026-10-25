<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		kind = 'error',
		children
	}: { kind?: 'error' | 'info' | 'success' | 'warning'; children: Snippet } = $props();

	const styles = {
		error: 'border-danger/40 bg-danger/10',
		info: 'border-accent/40 bg-accent/10',
		success: 'border-success/40 bg-success/10',
		warning: 'border-warning/50 bg-warning/10'
	};
</script>

<div
	role={kind === 'error' ? 'alert' : 'status'}
	class="rounded-card border px-4 py-3 text-sm {styles[kind]}"
>
	{@render children()}
</div>
