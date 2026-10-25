import { m } from '#lib/i18n/index.js';
import createClient from 'openapi-fetch';
import type { components, paths } from './schema';

export type Schemas = components['schemas'];
export type Me = Schemas['Me'];

/** Typed client for Katsanto's REST API (generated from the server's OpenAPI spec). */
export const api = createClient<paths>({ baseUrl: '', credentials: 'same-origin' });

/** A failed API call, with the server's machine-readable error code. */
export class ApiError extends Error {
	constructor(
		public status: number,
		public code: string,
		message: string
	) {
		super(message);
	}

	get isUnauthorized() {
		return this.status === 401;
	}

	get needsReauth() {
		return this.code === 'reauth_required';
	}
}

type Result<T> = { data?: T; error?: unknown; response: Response };

/** Unwrap an openapi-fetch result, throwing `ApiError` on failure. */
export async function unwrap<T>(call: Promise<Result<T>>): Promise<T> {
	let res: Result<T>;
	try {
		res = await call;
	} catch {
		throw new ApiError(0, 'network', m.errors.network);
	}
	if (res.response.ok) return res.data as T;
	const err = (res.error ?? {}) as { error?: string; message?: string };
	throw new ApiError(
		res.response.status,
		err.error ?? 'unknown',
		err.message ?? m.errors.status(res.response.status)
	);
}

/** Errors whose server text is generic, shown in the UI language instead. */
const GENERIC: Record<string, () => string> = {
	internal: () => m.errors.internal,
	reauth_required: () => m.errors.reauth
};

export function errorMessage(e: unknown): string {
	if (e instanceof ApiError) {
		const generic = GENERIC[e.code];
		return generic ? generic() : e.message;
	}
	if (e instanceof Error) {
		// WebAuthn cancellations surface as NotAllowedError.
		if (e.name === 'NotAllowedError') return m.auth.passkeyCancelled;
		return e.message;
	}
	return m.errors.unknown;
}
