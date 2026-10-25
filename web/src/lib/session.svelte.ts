import { goto } from '$app/navigation';
import { api, ApiError, unwrap, type Me } from '#lib/api/client.js';
import { appearance } from '#lib/appearance.svelte.js';
import { isLang, locale } from '#lib/i18n/index.js';

/** The logged-in user, shared by the whole app. */
class Session {
	me = $state<Me | null>(null);

	/** Load the current user; `null` for guests. */
	async load(): Promise<Me | null> {
		try {
			this.set(await unwrap(api.GET('/api/v1/me')));
		} catch (e) {
			if (e instanceof ApiError && e.isUnauthorized) this.me = null;
			else throw e;
		}
		return this.me;
	}

	set(me: Me) {
		// Settings already loaded for this user are the newest: this tab saves every change
		// it makes, and a refresh that started earlier must not undo one made meanwhile.
		if (this.me?.id === me.id) {
			this.me = { ...me, settings: this.me.settings };
			return;
		}
		this.me = me;
		appearance.fromServer(me.settings);
		const lang = (me.settings as Record<string, unknown> | undefined)?.language;
		if (isLang(lang)) locale.set(lang);
	}

	/** Change some settings and save them (the server stores the whole object). */
	saveSettings(patch: Record<string, unknown>) {
		if (!this.me) return;
		Object.assign(this.me.settings, patch);
		void api.PUT('/api/v1/me/settings', { body: { ...this.me.settings } });
	}

	async logout() {
		await unwrap(api.POST('/api/v1/auth/logout'));
		this.me = null;
		await goto('/login');
	}
}

export const session = new Session();

export { nextPath } from './nav.js';
