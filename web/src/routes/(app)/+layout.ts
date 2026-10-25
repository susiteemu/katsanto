import { redirect } from '@sveltejs/kit';
import { session } from '#lib/session.svelte.js';
import type { LayoutLoad } from './$types';

/** Every page in this group needs a logged-in user. */
export const load: LayoutLoad = async ({ url }) => {
	const me = session.me ?? (await session.load());
	if (!me) {
		const next = url.pathname + url.search;
		redirect(307, next === '/' ? '/login' : `/login?next=${encodeURIComponent(next)}`);
	}
	return { me };
};
