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

	const token = page.url.searchParams.get('token') ?? '';

	let info = $state<Schemas['ResetInfo'] | null>(null);
	let usePassword = $state(false);
	let password = $state('');
	let error = $state('');
	let busy = $state(false);
	let codes = $state<string[] | null>(null);
	let canPasskey = $derived(!!info?.passkeys_available && passkeysSupported());

	onMount(async () => {
		try {
			info = await unwrap(api.GET('/api/v1/auth/reset/{token}', { params: { path: { token } } }));
			if (!canPasskey) usePassword = true;
		} catch (e) {
			error = errorMessage(e);
		}
	});

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		busy = true;
		error = '';
		const path = { token };
		try {
			let result: Schemas['SignupResult'];
			if (usePassword) {
				result = await unwrap(
					api.POST('/api/v1/auth/reset/{token}/password', {
						params: { path },
						body: { password }
					})
				);
			} else {
				const challenge = await unwrap(
					api.POST('/api/v1/auth/reset/{token}/passkey/start', { params: { path } })
				);
				const credential = await createPasskey(challenge);
				result = await unwrap(
					api.POST('/api/v1/auth/reset/{token}/passkey/finish', {
						params: { path },
						body: { challenge_id: challenge.challenge_id, credential, name: suggestPasskeyName() }
					})
				);
			}
			session.set(result.user);
			codes = result.recovery_codes;
		} catch (err) {
			error = errorMessage(err);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>{m.signup.resetTitle}</title></svelte:head>

{#if codes}
	<h1 class="mb-2 text-xl font-semibold">{m.signup.saveNewCodes}</h1>
	<RecoveryCodes {codes} />
	<Button variant="primary" class="mt-6 w-full" onclick={() => goto('/')}
		>{m.signup.continue}</Button
	>
{:else}
	<h1 class="mb-1 text-xl font-semibold">{m.signup.resetHeading}</h1>
	<div class="flex flex-col gap-4">
		{#if error}<Alert>{error}</Alert>{/if}
		{#if info}
			<p class="text-sm text-muted">
				{m.signup.hi} <strong class="text-fg">{info.username}</strong>! {m.signup.resetIntro(
					usePassword
				)}
			</p>
			<form class="flex flex-col gap-4" onsubmit={submit}>
				{#if usePassword}
					<Field
						label={m.signup.newPassword}
						type="password"
						bind:value={password}
						autocomplete="new-password"
						minlength={12}
						required
					/>
				{/if}
				<Button type="submit" variant="primary" loading={busy}>
					{usePassword ? m.signup.setPassword : m.signup.createPasskey}
				</Button>
			</form>
			{#if canPasskey && info.passwords_enabled}
				<Button variant="ghost" onclick={() => (usePassword = !usePassword)}>
					{usePassword ? m.auth.usePasskey : m.auth.usePassword}
				</Button>
			{/if}
		{:else if !error}
			<p class="text-muted">{m.common.loading}</p>
		{/if}
	</div>
{/if}
