<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import Button from './Button.svelte';

	let { codes }: { codes: string[] } = $props();
	let copied = $state(false);

	async function copy() {
		await navigator.clipboard.writeText(codes.join('\n'));
		copied = true;
	}

	function download() {
		const text = `${m.recovery.fileHeader}\n\n${codes.join('\n')}\n`;
		const url = URL.createObjectURL(new Blob([text], { type: 'text/plain' }));
		const a = Object.assign(document.createElement('a'), {
			href: url,
			download: 'katsanto-recovery-codes.txt'
		});
		a.click();
		URL.revokeObjectURL(url);
	}
</script>

<div class="flex flex-col gap-4">
	<p class="text-sm text-muted">
		{m.recovery.intro}
	</p>
	<ul class="grid grid-cols-2 gap-2 rounded-card bg-surface-2 p-4 font-mono text-sm">
		{#each codes as code (code)}
			<li>{code}</li>
		{/each}
	</ul>
	<div class="flex flex-wrap gap-2">
		<Button size="sm" onclick={copy}>{copied ? m.recovery.copied : m.recovery.copy}</Button>
		<Button size="sm" onclick={download}>{m.recovery.download}</Button>
	</div>
</div>
