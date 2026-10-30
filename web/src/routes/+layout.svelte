<script lang="ts">
	import './layout.css';
	import favicon from '#lib/assets/favicon.svg';
	import { appearance } from '#lib/appearance.svelte.js';
	import { beforeNavigate } from '$app/navigation';
	import { updated } from '$app/state';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	$effect(() => appearance.apply());

	// SvelteKit notices a new deploy when the app comes back to the foreground (an iOS
	// home-screen app is resumed, not reloaded), but keeps running the old code. Load the
	// new version on the next navigation instead.
	beforeNavigate(({ willUnload, to }) => {
		if (updated.current && !willUnload && to?.url) location.href = to.url.href;
	});
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

{@render children()}
