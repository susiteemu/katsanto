<script lang="ts">
	import type { Snippet } from 'svelte';

	/**
	 * A small popup menu behind a "⋮" button. Items are buttons with
	 * `role="menuitem"`; clicking one closes the menu.
	 */
	let { label, children }: { label: string; children: Snippet } = $props();
	let open = $state(false);
	let root: HTMLDivElement;
	let button: HTMLButtonElement;
	let menu = $state<HTMLDivElement>();

	/** Gap between the button and the menu, and the menu and the viewport edge. */
	const GAP = 4;
	const EDGE = 8;

	// The menu is a popover in the top layer, so scrolling rows (overflow) and stacking
	// contexts can't clip or cover it. Place it under the button, right-aligned, or above
	// the button when there's no room below.
	$effect(() => {
		if (!menu) return;
		menu.showPopover();
		const b = button.getBoundingClientRect();
		const { width, height } = menu.getBoundingClientRect();
		const below = b.bottom + GAP;
		const top =
			below + height <= window.innerHeight - EDGE ? below : Math.max(EDGE, b.top - GAP - height);
		const left = Math.min(Math.max(EDGE, b.right - width), window.innerWidth - width - EDGE);
		menu.style.top = `${top}px`;
		menu.style.left = `${left}px`;
	});

	// A fixed-position menu would drift from its button when anything scrolls (the page or
	// a scrolling row, whose events don't bubble): close it instead.
	$effect(() => {
		if (!open) return;
		const close = () => (open = false);
		window.addEventListener('scroll', close, { capture: true, passive: true });
		return () => window.removeEventListener('scroll', close, { capture: true });
	});

	function onWindowClick(e: MouseEvent) {
		if (open && !root.contains(e.target as Node)) open = false;
	}
</script>

<svelte:window
	onclick={onWindowClick}
	onkeydown={(e) => e.key === 'Escape' && (open = false)}
	onresize={() => (open = false)}
/>

<div class="relative" bind:this={root}>
	<button
		bind:this={button}
		type="button"
		class="grid size-8 place-items-center rounded-full text-muted hover:bg-surface-2 hover:text-fg"
		aria-label={label}
		aria-haspopup="menu"
		aria-expanded={open}
		onclick={() => (open = !open)}
	>
		<svg viewBox="0 0 24 24" class="size-5" fill="currentColor" aria-hidden="true"
			><circle cx="12" cy="5" r="1.8" /><circle cx="12" cy="12" r="1.8" /><circle
				cx="12"
				cy="19"
				r="1.8"
			/></svg
		>
	</button>
	{#if open}
		<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
		<div
			bind:this={menu}
			popover="manual"
			role="menu"
			tabindex="-1"
			class="fixed inset-auto m-0 flex w-max min-w-44 flex-col overflow-hidden rounded-card border border-border bg-surface px-0 py-1 text-sm text-fg shadow-card [&_button]:px-4 [&_button]:py-2 [&_button]:text-left [&_button:hover]:bg-surface-2"
			onclick={() => (open = false)}
		>
			{@render children()}
		</div>
	{/if}
</div>
