<script lang="ts">
	import Logo from '#lib/components/Logo.svelte';
	import { m } from '#lib/i18n/index.js';
	import { page } from '$app/state';
	import { session } from '#lib/session.svelte.js';
	import Icon from '#lib/components/Icon.svelte';
	import SaveToListDialog from '#lib/components/SaveToListDialog.svelte';
	import SearchBox from '#lib/components/SearchBox.svelte';
	import ShortcutsDialog from '#lib/components/ShortcutsDialog.svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	type Item = {
		href: string;
		label: string;
		icon: 'home' | 'search' | 'subs' | 'list' | 'settings' | 'shield';
		/** Label in the mobile tab bar, when the full one is too long. */
		short?: string;
	};
	let items = $derived<Item[]>([
		{ href: '/', label: m.nav.home, icon: 'home' },
		{ href: '/search', label: m.nav.search, icon: 'search' },
		{ href: '/subscriptions', label: m.nav.subscriptions, icon: 'subs', short: m.nav.subsShort },
		{ href: '/lists', label: m.nav.lists, icon: 'list' },
		{ href: '/settings', label: m.nav.settings, icon: 'settings' },
		...(session.me?.is_admin
			? [{ href: '/admin', label: m.nav.admin, icon: 'shield' } as Item]
			: [])
	]);

	const active = (href: string) =>
		href === '/' ? page.url.pathname === '/' : page.url.pathname.startsWith(href);
</script>

<div class="min-h-dvh md:flex">
	<!-- Desktop sidebar -->
	<nav
		class="sticky top-0 hidden h-dvh w-60 shrink-0 flex-col gap-1 border-r border-border bg-surface p-4 md:flex"
		aria-label={m.nav.main}
	>
		<a href="/" class="mb-6 flex items-center gap-2 px-2">
			<Logo class="size-8" />
			<span class="text-lg font-semibold tracking-tight">Katsanto</span>
		</a>
		{#each items as item (item.href)}
			<a
				href={item.href}
				aria-current={active(item.href) ? 'page' : undefined}
				class="flex items-center gap-3 rounded-card px-3 py-2 text-muted transition hover:bg-surface-2 hover:text-fg aria-[current=page]:bg-surface-2 aria-[current=page]:font-medium aria-[current=page]:text-fg"
			>
				<Icon name={item.icon} />
				{item.label}
			</a>
		{/each}
		<div class="mt-auto flex items-center justify-between gap-2 border-t border-border px-2 pt-4">
			<span class="truncate text-sm text-muted">{session.me?.username}</span>
			<button
				class="rounded-card p-2 text-muted hover:bg-surface-2 hover:text-fg"
				onclick={() => session.logout()}
				title={m.nav.logOut}
				aria-label={m.nav.logOut}><Icon name="logout" size={18} /></button
			>
		</div>
	</nav>

	<main class="min-w-0 flex-1 px-4 pt-4 pb-24 md:px-8 md:pt-0 md:pb-10">
		<header
			class="sticky top-0 z-10 -mx-8 mb-6 hidden items-center justify-center bg-bg/90 px-8 py-3 backdrop-blur md:flex"
		>
			<SearchBox />
		</header>
		{@render children()}
	</main>

	<!-- Mobile bottom navigation -->
	<nav
		class="fixed inset-x-0 bottom-0 z-10 flex border-t border-border bg-surface/95 pb-[env(safe-area-inset-bottom)] backdrop-blur md:hidden"
		aria-label={m.nav.main}
	>
		{#each items.filter((i) => i.href !== '/admin') as item (item.href)}
			<a
				href={item.href}
				aria-current={active(item.href) ? 'page' : undefined}
				class="flex flex-1 flex-col items-center gap-0.5 py-2 text-xs text-muted aria-[current=page]:text-accent"
			>
				<Icon name={item.icon} size={22} />
				{item.short ?? item.label}
			</a>
		{/each}
	</nav>
</div>

<SaveToListDialog />
<ShortcutsDialog />
