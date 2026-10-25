<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import { createPasskey, passkeysSupported, suggestPasskeyName } from '#lib/passkeys.js';
	import { session } from '#lib/session.svelte.js';
	import Alert from '#lib/components/Alert.svelte';
	import Button from '#lib/components/Button.svelte';
	import Field from '#lib/components/Field.svelte';
	import RecoveryCodes from '#lib/components/RecoveryCodes.svelte';

	const invite = page.url.searchParams.get('invite') ?? undefined;

	let config = $state<Schemas['AuthConfig'] | null>(null);
	let usePassword = $state(false);
	let username = $state('');
	let password = $state('');
	let error = $state('');
	let busy = $state(false);
	let codes = $state<string[] | null>(null);

	let canPasskey = $derived(!!config?.passkeys_available && passkeysSupported());
	let blocked = $derived.by(() => {
		if (!config || config.needs_setup) return '';
		if (invite && config.invite_valid === false) return m.signup.inviteInvalid;
		if (!invite && config.registration_mode === 'invite') return m.signup.needInvite;
		if (!invite && config.registration_mode === 'closed') return m.signup.closed;
		return '';
	});

	onMount(async () => {
		try {
			config = await unwrap(api.GET('/api/v1/auth/config', { params: { query: { invite } } }));
			if (!canPasskey) usePassword = true;
		} catch (e) {
			error = errorMessage(e);
		}
	});

	async function done(result: Schemas['SignupResult']) {
		session.set(result.user);
		codes = result.recovery_codes;
	}

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		busy = true;
		error = '';
		try {
			if (usePassword) {
				await done(
					await unwrap(
						api.POST('/api/v1/auth/register/password', { body: { username, password, invite } })
					)
				);
			} else {
				const challenge = await unwrap(
					api.POST('/api/v1/auth/register/passkey/start', { body: { username, invite } })
				);
				const credential = await createPasskey(challenge);
				await done(
					await unwrap(
						api.POST('/api/v1/auth/register/passkey/finish', {
							body: {
								challenge_id: challenge.challenge_id,
								credential,
								name: suggestPasskeyName()
							}
						})
					)
				);
			}
		} catch (err) {
			error = errorMessage(err);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>{m.signup.title}</title></svelte:head>

{#if codes}
	<h1 class="mb-2 text-xl font-semibold">{m.signup.saveCodes}</h1>
	<RecoveryCodes {codes} />
	<Button variant="primary" class="mt-6 w-full" onclick={() => goto('/')}
		>{m.signup.continue}</Button
	>
{:else}
	<h1 class="mb-1 text-xl font-semibold">
		{config?.needs_setup ? m.signup.setUp : m.signup.createAccount}
	</h1>
	{#if config?.needs_setup}
		<p class="mb-6 text-sm text-muted">{m.signup.firstAdmin}</p>
	{:else}
		<p class="mb-6 text-sm text-muted">{m.signup.noEmail}</p>
	{/if}

	<div class="flex flex-col gap-4">
		{#if error}<Alert>{error}</Alert>{/if}

		{#if !config}
			<p class="text-muted">{m.common.loading}</p>
		{:else if blocked}
			<Alert kind="info">{blocked}</Alert>
		{:else}
			<form class="flex flex-col gap-4" onsubmit={submit}>
				<Field
					label={m.common.username}
					bind:value={username}
					autocomplete="username"
					autocapitalize="none"
					spellcheck="false"
					minlength={3}
					maxlength={32}
					pattern="[A-Za-z0-9][A-Za-z0-9._\-]*"
					hint={m.signup.usernameHint}
					required
				/>
				{#if usePassword}
					<Field
						label={m.common.password}
						type="password"
						bind:value={password}
						autocomplete="new-password"
						minlength={12}
						hint={m.signup.passwordHint}
						required
					/>
				{/if}
				<Button type="submit" variant="primary" loading={busy}>
					{usePassword ? m.signup.createAccount : m.signup.createWithPasskey}
				</Button>
			</form>
			{#if canPasskey && config.passwords_enabled}
				<Button variant="ghost" onclick={() => (usePassword = !usePassword)}>
					{usePassword ? m.auth.usePasskey : m.auth.usePassword}
				</Button>
			{/if}
			{#if !config.passkeys_available}
				<p class="text-xs text-muted">
					{m.signup.noPasskeysServer}
				</p>
			{:else if !canPasskey}
				<p class="text-xs text-muted">{m.signup.noPasskeysBrowser}</p>
			{/if}
		{/if}

		{#if config && !config.needs_setup}
			<p class="mt-2 border-t border-border pt-4 text-center text-sm text-muted">
				{m.signup.haveAccount} <a href="/login" class="underline hover:text-fg">{m.auth.logIn}</a>
			</p>
		{/if}
	</div>
{/if}
