import { api, unwrap } from '#lib/api/client.js';

/** Which channels the user is subscribed to, for subscribe buttons everywhere. */
class Subscriptions {
	ids = $state(new Set<string>());
	#loaded: Promise<void> | null = null;

	/** Load once; later calls reuse the same request. */
	load(force = false) {
		if (!this.#loaded || force) {
			this.#loaded = unwrap(api.GET('/api/v1/subscriptions/ids')).then((ids) => {
				this.ids = new Set(ids);
			});
		}
		return this.#loaded;
	}

	has(channelId: string) {
		return this.ids.has(channelId);
	}

	async subscribe(channelId: string) {
		this.ids = new Set([...this.ids, channelId]);
		try {
			await unwrap(api.POST('/api/v1/subscriptions', { body: { channel_id: channelId } }));
		} catch (e) {
			this.#drop(channelId);
			throw e;
		}
	}

	async unsubscribe(channelIds: string[]) {
		const before = this.ids;
		channelIds.forEach((id) => this.#drop(id));
		try {
			await unwrap(api.POST('/api/v1/subscriptions/remove', { body: { channel_ids: channelIds } }));
		} catch (e) {
			this.ids = before;
			throw e;
		}
	}

	#drop(id: string) {
		const next = new Set(this.ids);
		next.delete(id);
		this.ids = next;
	}
}

export const subscriptions = new Subscriptions();
