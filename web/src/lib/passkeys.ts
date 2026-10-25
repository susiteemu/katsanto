import {
	browserSupportsWebAuthn,
	browserSupportsWebAuthnAutofill,
	startAuthentication,
	startRegistration,
	WebAuthnAbortService
} from '@simplewebauthn/browser';

/** A WebAuthn response as plain JSON, as the server's `.../finish` endpoints take it. */
export type CredentialJSON = Record<string, unknown>;

/** `{ challenge_id, options }` from the server's `.../start` endpoints. */
export interface Challenge {
	challenge_id: string;
	options: Record<string, unknown>;
}

export const passkeysSupported = () => browserSupportsWebAuthn();
export const autofillSupported = () => browserSupportsWebAuthnAutofill();

/** A name for a new passkey, guessed from the device. */
export function suggestPasskeyName(): string {
	const ua = navigator.userAgent;
	if (/iPhone/.test(ua)) return 'iPhone';
	if (/iPad/.test(ua)) return 'iPad';
	if (/Android/.test(ua)) return 'Android';
	if (/Mac OS X/.test(ua)) return 'Mac';
	if (/Windows/.test(ua)) return 'Windows';
	if (/Linux/.test(ua)) return 'Linux';
	return 'Passkey';
}

/** Run `navigator.credentials.create` with the server's registration options. */
export async function createPasskey(challenge: Challenge): Promise<CredentialJSON> {
	const publicKey = challenge.options.publicKey as Parameters<
		typeof startRegistration
	>[0]['optionsJSON'];
	return (await startRegistration({ optionsJSON: publicKey })) as unknown as CredentialJSON;
}

/**
 * Run `navigator.credentials.get`. With `autofill`, the request waits for the user
 * to pick a passkey from the browser's autofill on a `autocomplete="username webauthn"` input.
 */
export async function usePasskey(challenge: Challenge, autofill = false): Promise<CredentialJSON> {
	const publicKey = challenge.options.publicKey as Parameters<
		typeof startAuthentication
	>[0]['optionsJSON'];
	const res = await startAuthentication({ optionsJSON: publicKey, useBrowserAutofill: autofill });
	return res as unknown as CredentialJSON;
}

/** Abort a pending autofill request (e.g. before starting a button-triggered one). */
export function cancelPasskeyRequest() {
	WebAuthnAbortService.cancelCeremony();
}
