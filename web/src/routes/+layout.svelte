<script lang="ts">
	import './layout.css';
	import favicon from '#lib/assets/favicon.svg';
	import { appearance } from '#lib/appearance.svelte.js';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	$effect(() => appearance.apply());
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

{@render children()}
