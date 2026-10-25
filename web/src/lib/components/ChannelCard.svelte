<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import type { Schemas } from '#lib/api/client.js';
	import { compact } from '#lib/format.js';
	import SubscribeButton from './SubscribeButton.svelte';

	let { channel }: { channel: Schemas['ChannelCard'] } = $props();
</script>

<div
	class="flex items-center gap-4 rounded-card p-3 transition hover:bg-surface-2 sm:col-span-full"
>
	<a href="/channel/{channel.id}" class="flex min-w-0 flex-1 items-center gap-4">
		<img
			src={channel.avatar_url}
			alt=""
			class="size-16 shrink-0 rounded-full bg-surface-2 sm:size-20"
			loading="lazy"
		/>
		<div class="min-w-0">
			<h3 class="truncate text-lg font-semibold">{channel.name}</h3>
			<p class="truncate text-sm text-muted">
				{[
					channel.handle,
					channel.subscriber_count != null &&
						m.channel.subscribers(compact(channel.subscriber_count))
				]
					.filter(Boolean)
					.join(' · ')}
			</p>
			{#if channel.description}
				<p class="mt-1 line-clamp-2 text-sm text-muted">{channel.description}</p>
			{/if}
		</div>
	</a>
	<SubscribeButton channelId={channel.id} name={channel.name} />
</div>
