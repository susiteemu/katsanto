<script lang="ts">
	import { date } from '#lib/format.js';
	import { m } from '#lib/i18n/index.js';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import { session } from '#lib/session.svelte.js';
	import Alert from '#lib/components/Alert.svelte';
	import Button from '#lib/components/Button.svelte';
	import Card from '#lib/components/Card.svelte';
	import Field from '#lib/components/Field.svelte';
	import ReauthDialog, { withReauth } from '#lib/components/ReauthDialog.svelte';

	let users = $state<Schemas['UserInfo'][]>([]);
	let invites = $state<Schemas['InviteInfo'][]>([]);
	let settings = $state<Schemas['AdminSettings'] | null>(null);
	let error = $state('');
	let busy = $state('');
	/** A freshly created link, shown once. */
	let link = $state<{ label: string; url: string; expires_at: number } | null>(null);
	let copied = $state(false);
	let newUser = $state('');
	let inviteNote = $state('');
	let inviteDays = $state('7');

	onMount(() => {
		if (!session.me?.is_admin) return goto('/');
		return void refresh();
	});

	async function refresh() {
		try {
			[users, invites, settings] = await Promise.all([
				unwrap(api.GET('/api/v1/admin/users')),
				unwrap(api.GET('/api/v1/admin/invites')),
				unwrap(api.GET('/api/v1/admin/settings'))
			]);
		} catch (e) {
			error = errorMessage(e);
		}
	}

	async function act(key: string, fn: () => Promise<unknown>) {
		busy = key;
		error = '';
		try {
			await withReauth(fn);
			await refresh();
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = '';
		}
	}

	function show(label: string, l: Schemas['CreatedLink']) {
		link = { label, ...l };
		copied = false;
	}

	const createUser = (e: SubmitEvent) => {
		e.preventDefault();
		return act('create', async () => {
			const res = await unwrap(api.POST('/api/v1/admin/users', { body: { username: newUser } }));
			show(m.admin.setupLink(newUser), res.setup_link);
			newUser = '';
		});
	};

	const update = (u: Schemas['UserInfo'], body: Schemas['UpdateUser']) =>
		act(`u-${u.id}`, () =>
			unwrap(api.PATCH('/api/v1/admin/users/{id}', { params: { path: { id: u.id } }, body }))
		);

	const resetLink = (u: Schemas['UserInfo']) =>
		act(`r-${u.id}`, async () => {
			const res = await unwrap(
				api.POST('/api/v1/admin/users/{id}/reset-link', { params: { path: { id: u.id } } })
			);
			show(m.admin.resetLinkFor(u.username), res);
		});

	const deleteUser = (u: Schemas['UserInfo']) =>
		confirm(m.admin.confirmDeleteUser(u.username)) &&
		act(`d-${u.id}`, () =>
			unwrap(api.DELETE('/api/v1/admin/users/{id}', { params: { path: { id: u.id } } }))
		);

	const createInvite = (e: SubmitEvent) => {
		e.preventDefault();
		return act('invite', async () => {
			const res = await unwrap(
				api.POST('/api/v1/admin/invites', {
					body: { note: inviteNote, expires_in_days: Number(inviteDays) }
				})
			);
			show(inviteNote ? m.admin.inviteFor(inviteNote) : m.admin.inviteLink, res.link);
			inviteNote = '';
		});
	};

	const deleteInvite = (i: Schemas['InviteInfo']) =>
		act(`i-${i.id}`, () =>
			unwrap(api.DELETE('/api/v1/admin/invites/{id}', { params: { path: { id: i.id } } }))
		);

	const saveSettings = (body: Schemas['UpdateSettings']) =>
		act('settings', () => unwrap(api.PATCH('/api/v1/admin/settings', { body })));

	async function copy() {
		if (!link) return;
		await navigator.clipboard.writeText(link.url);
		copied = true;
	}

	const when = (t?: number | null) => (t ? date(t) : '—');
	const now = () => Date.now() / 1000;
	const usernameOf = (id?: number | null) =>
		users.find((u) => u.id === id)?.username ?? m.admin.someone;
</script>

<svelte:head><title>{m.admin.title}</title></svelte:head>

<ReauthDialog />

<div class="mx-auto flex max-w-4xl flex-col gap-6">
	<h1 class="text-2xl font-semibold">{m.admin.heading}</h1>
	{#if error}<Alert>{error}</Alert>{/if}

	{#if link}
		<Alert kind="success">
			<p class="mb-2 font-medium">{m.admin.validUntilLabel(link.label, when(link.expires_at))}</p>
			<div class="flex flex-wrap items-center gap-2">
				<code class="min-w-0 flex-1 rounded bg-surface px-2 py-1 text-xs break-all">{link.url}</code
				>
				<Button size="sm" onclick={copy}>{copied ? m.recovery.copied : m.recovery.copy}</Button>
				<Button size="sm" variant="ghost" onclick={() => (link = null)}>{m.lists.done}</Button>
			</div>
			<p class="mt-2 text-xs text-muted">{m.admin.shownOnce}</p>
		</Alert>
	{/if}

	{#if settings}
		<Card title={m.admin.registration}>
			<div class="flex flex-col gap-5">
				<fieldset>
					<legend class="mb-2 text-sm font-medium">{m.admin.whoCanRegister}</legend>
					<div class="flex flex-wrap gap-2">
						{#each ['invite', 'open', 'closed'] as const as mode (mode)}
							<label
								class="cursor-pointer rounded-card border border-border px-3 py-2 text-sm has-checked:border-accent has-checked:bg-accent/10"
							>
								<input
									type="radio"
									name="mode"
									class="sr-only"
									checked={settings.registration_mode === mode}
									onchange={() => saveSettings({ registration_mode: mode })}
								/>
								{m.admin.modes[mode]}
							</label>
						{/each}
					</div>
				</fieldset>
				<label class="flex items-start gap-3">
					<input
						type="checkbox"
						class="mt-1 size-4 accent-[var(--accent)]"
						checked={settings.passwords_enabled}
						disabled={!settings.passkeys_available}
						onchange={(e) => saveSettings({ passwords_enabled: e.currentTarget.checked })}
					/>
					<span>
						<span class="font-medium">{m.admin.allowPasswords}</span>
						<span class="block text-sm text-muted">
							{m.admin.allowPasswordsHint}
						</span>
					</span>
				</label>
				{#if settings.passkeys_unavailable_reason}
					<Alert kind="warning">{settings.passkeys_unavailable_reason}</Alert>
				{/if}
			</div>
		</Card>
	{/if}

	<Card title={m.admin.users}>
		{#snippet actions()}
			<form class="flex gap-2" onsubmit={createUser}>
				<input
					bind:value={newUser}
					placeholder={m.admin.newUsername}
					aria-label={m.admin.newUsername}
					required
					class="h-8 w-40 rounded-card border border-border bg-surface px-2 text-sm"
				/>
				<Button size="sm" type="submit" loading={busy === 'create'}>{m.common.create}</Button>
			</form>
		{/snippet}
		<ul class="divide-y divide-border">
			{#each users as u (u.id)}
				<li class="flex flex-wrap items-center gap-3 py-3">
					<div class="min-w-0 flex-1">
						<p class="font-medium">
							{u.username}
							{#if u.is_admin}<span class="ml-1 text-xs text-accent">{m.admin.adminBadge}</span
								>{/if}
							{#if u.disabled}<span class="ml-1 text-xs text-danger">{m.admin.disabledBadge}</span
								>{/if}
						</p>
						<p class="text-sm text-muted">
							{[
								m.admin.passkeys(u.passkey_count),
								u.has_password && m.admin.password,
								m.admin.lastLogin(when(u.last_login_at))
							]
								.filter(Boolean)
								.join(' · ')}
						</p>
					</div>
					<div class="flex flex-wrap gap-1">
						<Button
							size="sm"
							variant="ghost"
							loading={busy === `r-${u.id}`}
							onclick={() => resetLink(u)}
						>
							{m.admin.resetLink}
						</Button>
						{#if u.id !== session.me?.id}
							<Button
								size="sm"
								variant="ghost"
								onclick={() => update(u, { is_admin: !u.is_admin })}
							>
								{u.is_admin ? m.admin.removeAdmin : m.admin.makeAdmin}
							</Button>
							<Button
								size="sm"
								variant="ghost"
								onclick={() => update(u, { disabled: !u.disabled })}
							>
								{u.disabled ? m.admin.enable : m.admin.disable}
							</Button>
							<Button size="sm" variant="ghost" class="text-danger" onclick={() => deleteUser(u)}>
								{m.common.delete}
							</Button>
						{/if}
					</div>
				</li>
			{/each}
		</ul>
	</Card>

	<Card title={m.admin.invites} description={m.admin.invitesHint}>
		<form class="mb-4 flex flex-col gap-3 sm:flex-row sm:items-end" onsubmit={createInvite}>
			<div class="flex-1">
				<Field
					label={m.admin.forNote}
					bind:value={inviteNote}
					placeholder={m.admin.notePlaceholder}
					maxlength={100}
				/>
			</div>
			<label class="flex flex-col gap-1.5 text-sm font-medium">
				{m.admin.validFor}
				<select
					bind:value={inviteDays}
					class="h-11 rounded-card border border-border bg-surface px-3 font-normal"
				>
					{#each [1, 7, 30] as d (d)}<option value={String(d)}>{m.admin.days(d)}</option>{/each}
				</select>
			</label>
			<Button type="submit" variant="primary" loading={busy === 'invite'}
				>{m.admin.createInvite}</Button
			>
		</form>
		{#if invites.length}
			<ul class="divide-y divide-border">
				{#each invites as i (i.id)}
					<li class="flex items-center gap-3 py-3 text-sm">
						<div class="min-w-0 flex-1">
							<p class="font-medium">{i.note || m.admin.invite}</p>
							<p class="text-muted">
								{#if i.used_at}
									{m.admin.usedBy(usernameOf(i.used_by), when(i.used_at))}
								{:else if i.expires_at < now()}
									{m.admin.expired(when(i.expires_at))}
								{:else}
									{m.admin.validUntil(when(i.expires_at))}
								{/if}
							</p>
						</div>
						<Button size="sm" variant="ghost" onclick={() => deleteInvite(i)}
							>{m.common.delete}</Button
						>
					</li>
				{/each}
			</ul>
		{/if}
	</Card>
</div>
