<script lang="ts">
	import { date } from '#lib/format.js';
	import { LANGS, locale, m, type Lang } from '#lib/i18n/index.js';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import { createPasskey, passkeysSupported, suggestPasskeyName } from '#lib/passkeys.js';
	import { session } from '#lib/session.svelte.js';
	import Alert from '#lib/components/Alert.svelte';
	import AppearanceSettings from '#lib/components/AppearanceSettings.svelte';
	import Button from '#lib/components/Button.svelte';
	import Card from '#lib/components/Card.svelte';
	import CommunitySettings from '#lib/components/CommunitySettings.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import RecoveryCodes from '#lib/components/RecoveryCodes.svelte';
	import SubtitleSettings from '#lib/components/SubtitleSettings.svelte';
	import ReauthDialog, { withReauth } from '#lib/components/ReauthDialog.svelte';

	let config = $state<Schemas['AuthConfig'] | null>(null);
	let passkeys = $state<Schemas['PasskeyInfo'][]>([]);
	let sessions = $state<Schemas['SessionInfo'][]>([]);
	let newCodes = $state<string[] | null>(null);
	let newPassword = $state('');
	let error = $state('');
	let notice = $state('');
	let busy = $state('');

	const recovered = page.url.searchParams.has('recovered');
	let me = $derived(session.me!);
	let canAddPasskey = $derived(!!config?.passkeys_available && passkeysSupported());

	onMount(refresh);

	async function refresh() {
		try {
			[config, passkeys, sessions] = await Promise.all([
				unwrap(api.GET('/api/v1/auth/config', { params: { query: {} } })),
				unwrap(api.GET('/api/v1/me/passkeys')),
				unwrap(api.GET('/api/v1/me/sessions'))
			]);
			await session.load();
		} catch (e) {
			error = errorMessage(e);
		}
	}

	/** Run an action with a busy marker, error display and a refresh afterwards. */
	async function act(key: string, fn: () => Promise<unknown>, done?: string) {
		busy = key;
		error = '';
		notice = '';
		try {
			const res = await withReauth(fn);
			if (res !== undefined && done) notice = done;
			await refresh();
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = '';
		}
	}

	const addPasskey = () =>
		act(
			'add',
			async () => {
				const challenge = await unwrap(api.POST('/api/v1/me/passkeys/start'));
				const credential = await createPasskey(challenge);
				return unwrap(
					api.POST('/api/v1/me/passkeys/finish', {
						body: { challenge_id: challenge.challenge_id, credential, name: suggestPasskeyName() }
					})
				);
			},
			m.settings.passkeyAdded
		);

	const renamePasskey = (p: Schemas['PasskeyInfo']) => {
		const name = prompt(m.settings.namePasskey, p.name)?.trim();
		if (!name || name === p.name) return;
		return act(`rename-${p.id}`, () =>
			unwrap(
				api.PATCH('/api/v1/me/passkeys/{id}', { params: { path: { id: p.id } }, body: { name } })
			)
		);
	};

	const deletePasskey = (p: Schemas['PasskeyInfo']) => {
		if (!confirm(m.settings.confirmRemovePasskey(p.name))) return;
		return act(
			`del-${p.id}`,
			() =>
				unwrap(api.DELETE('/api/v1/me/passkeys/{id}', { params: { path: { id: p.id } } })).then(
					() => true
				),
			m.settings.passkeyRemoved
		);
	};

	const setPassword = (e: SubmitEvent) => {
		e.preventDefault();
		return act(
			'password',
			async () => {
				await unwrap(api.PUT('/api/v1/me/password', { body: { password: newPassword } }));
				newPassword = '';
				return true;
			},
			m.settings.passwordSaved
		);
	};

	const removePassword = () =>
		confirm(m.settings.confirmRemovePassword) &&
		act(
			'rm-password',
			() => unwrap(api.DELETE('/api/v1/me/password')).then(() => true),
			m.settings.passwordRemoved
		);

	const regenerateCodes = () =>
		confirm(m.settings.confirmNewCodes) &&
		act('codes', async () => {
			const res = await unwrap(api.POST('/api/v1/me/recovery-codes'));
			newCodes = res.recovery_codes;
			return res;
		});

	const revoke = (s: Schemas['SessionInfo']) =>
		act(`s-${s.id}`, () =>
			unwrap(api.DELETE('/api/v1/me/sessions/{id}', { params: { path: { id: s.id } } }))
		);

	const signOutOthers = () =>
		act(
			'others',
			() => unwrap(api.DELETE('/api/v1/me/sessions')).then(() => true),
			m.settings.signedOutOthers
		);

	const deleteAccount = () =>
		confirm(m.settings.confirmDelete) &&
		act('delete', async () => {
			await unwrap(api.DELETE('/api/v1/me'));
			session.me = null;
			await goto('/login');
		});

	let restoreInput: HTMLInputElement;

	async function restore(e: Event) {
		const file = (e.currentTarget as HTMLInputElement).files?.[0];
		if (!file) return;
		busy = 'restore';
		error = '';
		notice = '';
		try {
			let body: Record<string, unknown>;
			try {
				body = JSON.parse(await file.text());
			} catch {
				throw new Error(m.settings.badBackup);
			}
			const res = await unwrap(api.POST('/api/v1/me/backup', { body }));
			notice = m.settings.restored(res);
			await session.load();
		} catch (err) {
			error = errorMessage(err);
		} finally {
			busy = '';
			restoreInput.value = '';
		}
	}

	const when = (t?: number | null) => (t ? date(t) : m.settings.never);

	function setLanguage(lang: Lang) {
		locale.set(lang);
		session.saveSettings({ language: lang });
	}

	/** A readable device name from a user agent string. */
	function device(ua: string) {
		const browser = /Edg\//.test(ua)
			? 'Edge'
			: /Firefox\//.test(ua)
				? 'Firefox'
				: /Chrome\//.test(ua)
					? 'Chrome'
					: /Safari\//.test(ua)
						? 'Safari'
						: m.settings.browser;
		const os = /iPhone|iPad/.test(ua)
			? 'iOS'
			: /Android/.test(ua)
				? 'Android'
				: /Mac OS X/.test(ua)
					? 'macOS'
					: /Windows/.test(ua)
						? 'Windows'
						: /Linux/.test(ua)
							? 'Linux'
							: '';
		return os ? m.settings.deviceOn(browser, os) : browser || m.settings.unknownDevice;
	}
</script>

<svelte:head><title>{m.settings.title}</title></svelte:head>

<ReauthDialog />

<div class="mx-auto flex max-w-3xl flex-col gap-6">
	<h1 class="text-2xl font-semibold">{m.settings.heading}</h1>

	{#if recovered}
		<Alert kind="warning">
			{m.settings.recovered}
		</Alert>
	{/if}
	{#if error}<Alert>{error}</Alert>{/if}
	{#if notice}<Alert kind="success">{notice}</Alert>{/if}

	<Card title={m.settings.account}>
		<div class="flex items-center justify-between gap-3">
			<p>
				{m.settings.loggedInAs} <strong>{me.username}</strong>{#if me.is_admin}<span
						class="ml-1 text-sm text-accent">{m.settings.adminBadge}</span
					>{/if}
			</p>
			<div class="flex gap-2">
				{#if me.is_admin}
					<Button href="/admin" size="sm" class="md:hidden">{m.nav.admin}</Button>
				{/if}
				<Button size="sm" onclick={() => session.logout()}
					><Icon name="logout" size={16} /> {m.nav.logOut}</Button
				>
			</div>
		</div>
	</Card>

	<Card title={m.settings.language} description={m.settings.languageHint}>
		<select
			value={locale.current}
			onchange={(e) => setLanguage(e.currentTarget.value as Lang)}
			aria-label={m.settings.language}
			class="h-10 rounded-card border border-border bg-surface px-3"
		>
			{#each LANGS as l (l.id)}<option value={l.id} lang={l.id}>{l.label}</option>{/each}
		</select>
	</Card>

	<AppearanceSettings />

	<SubtitleSettings />

	<CommunitySettings />

	<Card title={m.settings.passkeys} description={m.settings.passkeysHint}>
		{#snippet actions()}
			{#if canAddPasskey}
				<Button size="sm" variant="primary" loading={busy === 'add'} onclick={addPasskey}>
					<Icon name="plus" size={16} />
					{m.settings.addPasskey}
				</Button>
			{/if}
		{/snippet}
		{#if config && !config.passkeys_available}
			<p class="text-sm text-muted">{m.settings.noPasskeysServer}</p>
		{:else if passkeys.length === 0}
			<p class="text-sm text-muted">{m.settings.noPasskeys}</p>
		{:else}
			<ul class="divide-y divide-border">
				{#each passkeys as p (p.id)}
					<li class="flex items-center gap-3 py-3">
						<Icon name="key" class="text-muted" />
						<div class="min-w-0 flex-1">
							<p class="truncate font-medium">{p.name}</p>
							<p class="text-sm text-muted">
								{m.settings.passkeyDates(when(p.created_at), when(p.last_used_at))}
							</p>
						</div>
						<Button size="sm" variant="ghost" onclick={() => renamePasskey(p)}
							>{m.settings.rename}</Button
						>
						<Button
							size="sm"
							variant="ghost"
							loading={busy === `del-${p.id}`}
							onclick={() => deletePasskey(p)}
							aria-label={m.common.removeItem(p.name)}><Icon name="trash" size={16} /></Button
						>
					</li>
				{/each}
			</ul>
		{/if}
	</Card>

	{#if config?.passwords_enabled || me.has_password}
		<Card
			title={m.settings.password}
			description={me.has_password ? m.settings.hasPassword : m.settings.passwordFallback}
		>
			<form class="flex flex-col gap-3 sm:flex-row sm:items-end" onsubmit={setPassword}>
				<div class="flex-1">
					<Field
						label={me.has_password ? m.signup.newPassword : m.common.password}
						type="password"
						bind:value={newPassword}
						autocomplete="new-password"
						minlength={12}
						required
					/>
				</div>
				<Button type="submit" variant="primary" loading={busy === 'password'}>
					{me.has_password ? m.settings.change : m.signup.setPassword}
				</Button>
			</form>
			{#if me.has_password && me.passkey_count > 0}
				<Button
					class="mt-3"
					size="sm"
					variant="ghost"
					loading={busy === 'rm-password'}
					onclick={removePassword}
				>
					{m.settings.removePassword}
				</Button>
			{/if}
		</Card>
	{/if}

	<Card
		title={m.settings.recoveryCodes}
		description={m.settings.codesLeft(me.recovery_codes_remaining)}
	>
		{#if newCodes}
			<RecoveryCodes codes={newCodes} />
		{:else}
			<Button loading={busy === 'codes'} onclick={regenerateCodes}>{m.settings.newCodes}</Button>
		{/if}
	</Card>

	<Card title={m.settings.sessions} description={m.settings.sessionsHint}>
		{#snippet actions()}
			{#if sessions.length > 1}
				<Button size="sm" loading={busy === 'others'} onclick={signOutOthers}
					>{m.settings.signOutOthers}</Button
				>
			{/if}
		{/snippet}
		<ul class="divide-y divide-border">
			{#each sessions as s (s.id)}
				<li class="flex items-center gap-3 py-3">
					<div class="min-w-0 flex-1">
						<p class="font-medium">
							{device(s.user_agent)}
							{#if s.current}<span class="ml-1 text-sm text-accent">· {m.settings.thisDevice}</span
								>{/if}
						</p>
						<p class="text-sm text-muted">{m.settings.lastActive(when(s.last_seen_at))}</p>
					</div>
					{#if !s.current}
						<Button
							size="sm"
							variant="ghost"
							loading={busy === `s-${s.id}`}
							onclick={() => revoke(s)}
						>
							{m.settings.signOut}
						</Button>
					{/if}
				</li>
			{/each}
		</ul>
	</Card>

	<Card title={m.settings.backup} description={m.settings.backupHint}>
		<div class="flex flex-wrap items-center gap-2">
			<Button href="/api/v1/me/backup" variant="primary">{m.settings.download}</Button>
			<Button loading={busy === 'restore'} onclick={() => restoreInput.click()}
				>{m.settings.restore}</Button
			>
			<input
				bind:this={restoreInput}
				type="file"
				accept="application/json,.json"
				class="hidden"
				onchange={restore}
				aria-label={m.settings.restore}
			/>
		</div>
		<p class="mt-3 text-sm text-muted">{m.settings.restoreHint}</p>
	</Card>

	<Card title={m.settings.deleteAccount} description={m.settings.deleteHint}>
		<Button variant="danger" loading={busy === 'delete'} onclick={deleteAccount}
			>{m.settings.deleteMine}</Button
		>
	</Card>
</div>
