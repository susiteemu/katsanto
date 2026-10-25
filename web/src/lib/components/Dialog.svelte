<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		open = $bindable(false),
		title,
		onclose,
		children
	}: { open?: boolean; title: string; onclose?: () => void; children: Snippet } = $props();

	let dialog: HTMLDialogElement;
	const titleId = $props.id();

	$effect(() => {
		if (open && !dialog.open) dialog.showModal();
		if (!open && dialog.open) dialog.close();
	});
</script>

<dialog
	bind:this={dialog}
	onclose={() => {
		open = false;
		onclose?.();
	}}
	class="m-auto w-[min(28rem,calc(100vw-2rem))] rounded-card border border-border bg-surface p-0 text-fg shadow-card backdrop:bg-black/50"
	aria-labelledby={titleId}
>
	<div class="p-5">
		<h2 id={titleId} class="mb-4 text-lg font-semibold">{title}</h2>
		{@render children()}
	</div>
</dialog>
