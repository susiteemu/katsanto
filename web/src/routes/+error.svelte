<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { page } from '$app/state';
	import Button from '#lib/components/Button.svelte';

	let online = $state(typeof navigator === 'undefined' || navigator.onLine);
</script>

<svelte:window ononline={() => (online = true)} onoffline={() => (online = false)} />
<svelte:head><title>{online ? m.error.errorTitle : m.error.offlineTitle}</title></svelte:head>

<main class="grid min-h-dvh place-items-center p-6">
	<div class="flex max-w-sm flex-col items-center gap-4 text-center">
		<h1 class="text-2xl font-semibold">
			{#if !online}{m.error.offline}{:else if page.status === 404}{m.error.notFound}{:else}{m.error
					.failed}{/if}
		</h1>
		<p class="text-muted">
			{#if !online}
				{m.error.offlineHint}
			{:else}
				{page.error?.message ?? m.error.unexpected}
			{/if}
		</p>
		<Button variant="primary" onclick={() => location.reload()}>{m.common.retry}</Button>
	</div>
</main>
