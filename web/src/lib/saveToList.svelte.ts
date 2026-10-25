import { api, unwrap } from '#lib/api/client.js';

/** Opens the app-wide "Save to list" dialog (rendered once in the app layout). */
class SaveToList {
	video = $state<{ id: string; title: string } | null>(null);

	open(id: string, title: string) {
		this.video = { id, title };
	}

	close() {
		this.video = null;
	}

	/** One-click save to Watch later. */
	async watchLater(id: string) {
		const lists = await unwrap(api.GET('/api/v1/lists'));
		const wl = lists.find((l) => l.kind === 'watch_later')!;
		await unwrap(
			api.POST('/api/v1/lists/{id}/items', {
				params: { path: { id: wl.id } },
				body: { video_id: id }
			})
		);
	}
}

export const saveToList = new SaveToList();
