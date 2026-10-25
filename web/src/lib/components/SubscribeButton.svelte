<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { onMount } from 'svelte';
	import { errorMessage } from '#lib/api/client.js';
	import { subscriptions } from '#lib/subscriptions.svelte.js';

	let { channelId, name = '' }: { channelId: string; name?: string } = $props();
	let busy = $state(false);
	const subscribed = $derived(subscriptions.has(channelId));

	onMount(() => void subscriptions.load());

	async function toggle() {
		if (subscribed && !confirm(m.channel.confirmUnsubscribe(name || m.channel.thisChannel))) return;
		busy = true;
		try {
			if (subscribed) await subscriptions.unsubscribe([channelId]);
			else await subscriptions.subscribe(channelId);
		} catch (e) {
			alert(errorMessage(e));
		} finally {
			busy = false;
		}
	}
</script>

<button
	class="h-9 shrink-0 rounded-full px-4 text-sm font-medium transition disabled:opacity-60 {subscribed
		? 'bg-surface-2 text-fg hover:bg-border'
		: 'bg-fg text-bg hover:opacity-85'}"
	onclick={toggle}
	disabled={busy}
	aria-pressed={subscribed}
>
	{subscribed ? m.channel.subscribed : m.channel.subscribe}
</button>
