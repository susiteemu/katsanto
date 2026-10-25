<script lang="ts">
	import type { Schemas } from '#lib/api/client.js';

	interface Props {
		parts: Schemas['TextPart'][];
		/** The video being watched: timestamps into it seek instead of navigating. */
		videoId: string;
		onseek: (seconds: number) => void;
	}

	let { parts, videoId, onseek }: Props = $props();
</script>

<!-- Plain text keeps its line breaks; links stay inside Katsanto where possible. -->
<p class="text-sm break-words whitespace-pre-wrap">
	{#each parts as part, i (i)}
		{#if part.type === 'text'}{part.text}{:else if part.type === 'link'}<a
				href={part.url}
				target="_blank"
				rel="noopener noreferrer nofollow"
				class="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-current"
				>{part.text}</a
			>{:else if part.type === 'video' && part.id === videoId}<button
				class="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-current"
				onclick={() => onseek(part.start_s)}>{part.text}</button
			>{:else if part.type === 'video'}<a
				href="/watch/{part.id}{part.start_s ? `?t=${part.start_s}` : ''}"
				class="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-current"
				>{part.text}</a
			>{:else if part.type === 'channel'}<a
				href="/channel/{part.id}"
				class="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-current"
				>{part.text}</a
			>{/if}
	{/each}
</p>
