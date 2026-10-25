<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, unwrap } from '#lib/api/client.js';
	import Icon from './Icon.svelte';

	let query = $state(page.url.pathname === '/search' ? (page.url.searchParams.get('q') ?? '') : '');
	let suggestions = $state<string[]>([]);
	let active = $state(-1);
	let open = $state(false);
	let input: HTMLInputElement;
	let timer: ReturnType<typeof setTimeout>;
	let latest = 0;

	const isUrl = (s: string) =>
		/^(https?:\/\/)?((www|m|music)\.)?(youtube\.com|youtu\.be)\//i.test(s);

	function oninput() {
		clearTimeout(timer);
		active = -1;
		const q = query.trim();
		if (q.length < 2 || isUrl(q)) {
			suggestions = [];
			return;
		}
		timer = setTimeout(async () => {
			const req = ++latest;
			const { data } = await api.GET('/api/v1/search/suggestions', { params: { query: { q } } });
			// Ignore answers to older keystrokes.
			if (req === latest) {
				suggestions = (data ?? []).slice(0, 8);
				open = true;
			}
		}, 200);
	}

	async function submit(text = query) {
		const q = text.trim();
		if (!q) return;
		open = false;
		query = q;
		input.blur();
		if (isUrl(q)) {
			try {
				const url = q.startsWith('http') ? q : `https://${q}`;
				const r = await unwrap(api.GET('/api/v1/resolve', { params: { query: { url } } }));
				if (r.type === 'video') return goto(`/watch/${r.id}${r.start_s ? `?t=${r.start_s}` : ''}`);
				if (r.type === 'channel') return goto(`/channel/${r.id}`);
			} catch {
				// Not resolvable: search for it instead.
			}
		}
		const params = new URLSearchParams(page.url.pathname === '/search' ? page.url.search : '');
		params.set('q', q);
		await goto(`/search?${params}`);
	}

	function onkeydown(e: KeyboardEvent) {
		if (!open || suggestions.length === 0) return;
		if (e.key === 'ArrowDown') {
			e.preventDefault();
			active = (active + 1) % suggestions.length;
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			active = (active - 1 + suggestions.length) % suggestions.length;
		} else if (e.key === 'Escape') {
			open = false;
		} else if (e.key === 'Enter' && active >= 0) {
			e.preventDefault();
			void submit(suggestions[active]);
		}
	}

	/** `/` focuses search from anywhere, like YouTube. */
	function onWindowKey(e: KeyboardEvent) {
		const t = e.target as HTMLElement;
		if (
			e.key === '/' &&
			!e.ctrlKey &&
			!e.metaKey &&
			!/INPUT|TEXTAREA|SELECT/.test(t.tagName) &&
			!t.isContentEditable
		) {
			e.preventDefault();
			input.focus();
		}
	}
</script>

<svelte:window onkeydown={onWindowKey} />

<form
	role="search"
	class="relative w-full max-w-2xl"
	onsubmit={(e) => {
		e.preventDefault();
		void submit();
	}}
>
	<Icon
		name="search"
		size={18}
		class="pointer-events-none absolute top-1/2 left-3.5 -translate-y-1/2 text-muted"
	/>
	<input
		bind:this={input}
		bind:value={query}
		{oninput}
		{onkeydown}
		onfocus={() => (open = suggestions.length > 0)}
		onblur={() => setTimeout(() => (open = false), 150)}
		type="search"
		placeholder={m.nav.searchPlaceholder}
		aria-label={m.nav.searchLabel}
		role="combobox"
		aria-expanded={open && suggestions.length > 0}
		aria-controls="search-suggestions"
		aria-activedescendant={active >= 0 ? `suggestion-${active}` : undefined}
		autocomplete="off"
		class="h-10 w-full rounded-full border border-border bg-surface pr-4 pl-10 focus:border-accent focus:outline-none"
	/>
	{#if open && suggestions.length > 0}
		<ul
			id="search-suggestions"
			role="listbox"
			class="absolute inset-x-0 top-12 z-20 overflow-hidden rounded-card border border-border bg-surface py-1 shadow-card"
		>
			{#each suggestions as s, i (s)}
				<li
					id="suggestion-{i}"
					role="option"
					aria-selected={i === active}
					class="flex cursor-pointer items-center gap-3 px-4 py-2 aria-selected:bg-surface-2"
					onmousedown={(e) => {
						e.preventDefault();
						void submit(s);
					}}
				>
					<Icon name="search" size={16} class="text-muted" />{s}
				</li>
			{/each}
		</ul>
	{/if}
</form>
