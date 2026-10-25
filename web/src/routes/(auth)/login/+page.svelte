<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { onDestroy, onMount } from 'svelte';
	import { api, errorMessage, unwrap, type Schemas } from '#lib/api/client.js';
	import {
		autofillSupported,
		cancelPasskeyRequest,
		passkeysSupported,
		usePasskey
	} from '#lib/passkeys.js';
	import { nextPath, session } from '#lib/session.svelte.js';
	import Alert from '#lib/components/Alert.svelte';
	import Button from '#lib/components/Button.svelte';
	import Field from '#lib/components/Field.svelte';

	let config = $state<Schemas['AuthConfig'] | null>(null);
	let mode = $state<'main' | 'password' | 'recovery'>('main');
	let username = $state('');
	let password = $state('');
	let code = $state('');
	let error = $state('');
	let busy = $state(false);
	let canPasskey = $derived(!!config?.passkeys_available && passkeysSupported());

	onMount(async () => {
		try {
			if (await session.load()) return goto(nextPath(page.url));
			config = await unwrap(api.GET('/api/v1/auth/config', { params: { query: {} } }));
			if (config.needs_setup) return goto('/register');
			if (!canPasskey) mode = 'password';
			else if (await autofillSupported()) void passkeyLogin(true);
		} catch (e) {
			error = errorMessage(e);
		}
	});

	onDestroy(cancelPasskeyRequest);

	async function finish(result: Schemas['LoginResult']) {
		session.set(result.user);
		await goto(result.used_recovery_code ? '/settings?recovered=1' : nextPath(page.url));
	}

	/** With `autofill`, waits in the background for the user to pick a passkey from the username field. */
	async function passkeyLogin(autofill: boolean) {
		if (!autofill) {
			cancelPasskeyRequest();
			busy = true;
		}
		error = '';
		try {
			const challenge = await unwrap(api.POST('/api/v1/auth/login/passkey/start'));
			const credential = await usePasskey(challenge, autofill);
			busy = true;
			await finish(
				await unwrap(
					api.POST('/api/v1/auth/login/passkey/finish', {
						body: { challenge_id: challenge.challenge_id, credential }
					})
				)
			);
		} catch (e) {
			// An aborted autofill request is expected when the user clicks the button instead.
			if (!(autofill && e instanceof Error && e.name === 'AbortError')) error = errorMessage(e);
		} finally {
			busy = false;
		}
	}

	async function passwordLogin(e: SubmitEvent) {
		e.preventDefault();
		cancelPasskeyRequest();
		busy = true;
		error = '';
		try {
			await finish(
				await unwrap(api.POST('/api/v1/auth/login/password', { body: { username, password } }))
			);
		} catch (err) {
			error = errorMessage(err);
		} finally {
			busy = false;
		}
	}

	async function recoveryLogin(e: SubmitEvent) {
		e.preventDefault();
		cancelPasskeyRequest();
		busy = true;
		error = '';
		try {
			await finish(
				await unwrap(api.POST('/api/v1/auth/login/recovery', { body: { username, code } }))
			);
		} catch (err) {
			error = errorMessage(err);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>{m.auth.loginTitle}</title></svelte:head>

<h1 class="mb-6 text-xl font-semibold">{m.auth.logIn}</h1>

<div class="flex flex-col gap-4">
	{#if error}<Alert>{error}</Alert>{/if}

	{#if !config}
		<p class="text-muted">{m.common.loading}</p>
	{:else if mode === 'main'}
		<!-- The username field lets the browser offer saved passkeys as autofill. -->
		<Field
			label={m.common.username}
			bind:value={username}
			autocomplete="username webauthn"
			autocapitalize="none"
			spellcheck="false"
		/>
		<Button variant="primary" loading={busy} onclick={() => passkeyLogin(false)}>
			{m.auth.withPasskey}
		</Button>
		{#if config.passwords_enabled}
			<Button variant="ghost" onclick={() => (mode = 'password')}>{m.auth.usePassword}</Button>
		{/if}
	{:else if mode === 'password'}
		<form class="flex flex-col gap-4" onsubmit={passwordLogin}>
			<Field
				label={m.common.username}
				bind:value={username}
				autocomplete="username"
				autocapitalize="none"
				spellcheck="false"
				required
			/>
			<Field
				label={m.common.password}
				type="password"
				bind:value={password}
				autocomplete="current-password"
				required
			/>
			<Button type="submit" variant="primary" loading={busy}>{m.auth.logIn}</Button>
		</form>
		{#if canPasskey}
			<Button variant="ghost" onclick={() => (mode = 'main')}>{m.auth.usePasskey}</Button>
		{/if}
	{:else}
		<form class="flex flex-col gap-4" onsubmit={recoveryLogin}>
			<Field
				label={m.common.username}
				bind:value={username}
				autocomplete="username"
				autocapitalize="none"
				required
			/>
			<Field
				label={m.auth.recoveryCode}
				bind:value={code}
				placeholder="XXXX-XXXX-XXXX"
				autocomplete="one-time-code"
				autocapitalize="characters"
				required
			/>
			<Button type="submit" variant="primary" loading={busy}>{m.auth.logIn}</Button>
		</form>
		<Button variant="ghost" onclick={() => (mode = canPasskey ? 'main' : 'password')}
			>{m.common.back}</Button
		>
	{/if}

	{#if config && mode !== 'recovery'}
		<div class="mt-2 border-t border-border pt-4 text-center text-sm text-muted">
			<button class="underline hover:text-fg" onclick={() => (mode = 'recovery')}>
				{m.auth.lostPasskey}
			</button>
		</div>
	{/if}
</div>
