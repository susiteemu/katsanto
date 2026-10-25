<script lang="ts" module>
	import { ApiError } from '#lib/api/client.js';

	let request = $state<{ resolve: (ok: boolean) => void } | null>(null);

	/**
	 * Run `action`; if the server asks for re-authentication, show the dialog and
	 * retry once confirmed. Resolves to `undefined` if the user cancels.
	 */
	export async function withReauth<T>(action: () => Promise<T>): Promise<T | undefined> {
		try {
			return await action();
		} catch (e) {
			if (!(e instanceof ApiError && e.needsReauth)) throw e;
			const ok = await new Promise<boolean>((resolve) => (request = { resolve }));
			return ok ? action() : undefined;
		}
	}
</script>

<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import { api, errorMessage, unwrap } from '#lib/api/client.js';
	import { passkeysSupported, usePasskey } from '#lib/passkeys.js';
	import { session } from '#lib/session.svelte.js';
	import Alert from './Alert.svelte';
	import Button from './Button.svelte';
	import Dialog from './Dialog.svelte';
	import Field from './Field.svelte';

	let password = $state('');
	let error = $state('');
	let busy = $state(false);
	let open = $derived(request !== null);
	let hasPasskey = $derived((session.me?.passkey_count ?? 0) > 0 && passkeysSupported());

	function close(ok: boolean) {
		request?.resolve(ok);
		request = null;
		password = '';
		error = '';
	}

	async function viaPasskey() {
		busy = true;
		error = '';
		try {
			const challenge = await unwrap(api.POST('/api/v1/auth/reauth/passkey/start'));
			const credential = await usePasskey(challenge);
			await unwrap(
				api.POST('/api/v1/auth/reauth/passkey/finish', {
					body: { challenge_id: challenge.challenge_id, credential }
				})
			);
			close(true);
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}

	async function viaPassword(e: SubmitEvent) {
		e.preventDefault();
		busy = true;
		error = '';
		try {
			await unwrap(api.POST('/api/v1/auth/reauth/password', { body: { password } }));
			close(true);
		} catch (err) {
			error = errorMessage(err);
		} finally {
			busy = false;
		}
	}
</script>

<Dialog {open} title={m.reauth.title}>
	<div class="flex flex-col gap-4">
		<p class="text-sm text-muted">{m.reauth.intro}</p>
		{#if error}<Alert>{error}</Alert>{/if}
		{#if hasPasskey}
			<Button variant="primary" loading={busy} onclick={viaPasskey}>{m.reauth.passkey}</Button>
		{/if}
		{#if session.me?.has_password}
			<form class="flex flex-col gap-3" onsubmit={viaPassword}>
				<Field
					label={m.common.password}
					type="password"
					bind:value={password}
					autocomplete="current-password"
					required
				/>
				<Button type="submit" variant={hasPasskey ? 'secondary' : 'primary'} loading={busy}>
					{m.reauth.password}
				</Button>
			</form>
		{/if}
		<Button variant="ghost" onclick={() => close(false)}>{m.common.cancel}</Button>
	</div>
</Dialog>
