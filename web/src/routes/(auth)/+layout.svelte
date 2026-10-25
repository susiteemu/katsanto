<script lang="ts">
	import Logo from '#lib/components/Logo.svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();
</script>

<main class="flex min-h-dvh items-center justify-center px-4 py-10">
	<div class="w-full max-w-sm">
		<div class="mb-8 flex items-center justify-center gap-2">
			<Logo class="size-9" />
			<span class="text-2xl font-semibold tracking-tight">Katsanto</span>
		</div>
		<div class="rounded-card border border-border bg-surface p-6 shadow-card">
			{@render children()}
		</div>
	</div>
</main>
