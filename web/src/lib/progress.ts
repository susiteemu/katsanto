import { api, type Schemas } from '#lib/api/client.js';

export type Progress = Schemas['ProgressInfo'];

/** Save while playing this often (seconds). */
const INTERVAL_S = 10;
const QUEUE_KEY = 'katsanto.progressQueue';

type Pending = Record<string, { position_s: number; duration_s: number }>;

/** The part of the media element API used here (native `<video>` or `<dash-video>`). */
export type MediaLike = Pick<
	HTMLMediaElement,
	| 'currentTime'
	| 'duration'
	| 'ended'
	| 'paused'
	| 'readyState'
	| 'play'
	| 'addEventListener'
	| 'removeEventListener'
>;

/** Save a position. `keepalive` lets the request finish while the page unloads. */
async function send(videoId: string, position_s: number, duration_s: number): Promise<boolean> {
	try {
		const res = await fetch(`/api/v1/progress/${encodeURIComponent(videoId)}`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ position_s, duration_s }),
			keepalive: true,
			credentials: 'same-origin'
		});
		// 4xx won't get better by retrying; only network/5xx errors are queued.
		return res.ok || (res.status >= 400 && res.status < 500);
	} catch {
		return false;
	}
}

function readQueue(): Pending {
	try {
		return JSON.parse(localStorage.getItem(QUEUE_KEY) ?? '{}');
	} catch {
		return {};
	}
}

function writeQueue(q: Pending) {
	try {
		if (Object.keys(q).length) localStorage.setItem(QUEUE_KEY, JSON.stringify(q));
		else localStorage.removeItem(QUEUE_KEY);
	} catch {
		// Storage unavailable: the latest position is simply lost.
	}
}

/** Retry saves that failed while offline. */
export async function flushQueue() {
	const q = readQueue();
	for (const [id, p] of Object.entries(q)) {
		if (await send(id, p.position_s, p.duration_s)) delete q[id];
	}
	writeQueue(q);
}

if (typeof window !== 'undefined') window.addEventListener('online', () => void flushQueue());

/**
 * Keep the server up to date with the playback position of `media`:
 * periodically while playing, on pause/seek/end, and when the page is hidden
 * or closed (mobile browsers often never fire `unload`). Returns a cleanup function.
 */
export function trackProgress(media: MediaLike, videoId: string): () => void {
	let lastSaved = -1;
	let lastSentAt = 0;

	const save = (force = false) => {
		const duration = media.duration;
		const position = media.ended ? duration : media.currentTime;
		if (!Number.isFinite(duration) || duration <= 0 || position < 1) return;
		if (!force && Math.abs(position - lastSaved) < 1) return;
		lastSaved = position;
		lastSentAt = Date.now();
		void send(videoId, position, duration).then((ok) => {
			if (!ok)
				writeQueue({ ...readQueue(), [videoId]: { position_s: position, duration_s: duration } });
		});
	};

	const tick = setInterval(() => {
		if (!media.paused && Date.now() - lastSentAt >= INTERVAL_S * 1000) save();
	}, 1000);
	const onPause = () => save();
	const onSeeked = () => save();
	const onEnded = () => save(true);
	const onHide = () => document.visibilityState === 'hidden' && save();
	const onPageHide = () => save();

	media.addEventListener('pause', onPause);
	media.addEventListener('seeked', onSeeked);
	media.addEventListener('ended', onEnded);
	document.addEventListener('visibilitychange', onHide);
	window.addEventListener('pagehide', onPageHide);
	void flushQueue();

	return () => {
		save();
		clearInterval(tick);
		media.removeEventListener('pause', onPause);
		media.removeEventListener('seeked', onSeeked);
		media.removeEventListener('ended', onEnded);
		document.removeEventListener('visibilitychange', onHide);
		window.removeEventListener('pagehide', onPageHide);
	};
}

/** Progress for a list of videos, keyed by video id (for card progress bars). */
export async function progressFor(ids: string[]): Promise<Map<string, Progress>> {
	const map = new Map<string, Progress>();
	for (let i = 0; i < ids.length; i += 100) {
		const chunk = ids.slice(i, i + 100);
		const { data } = await api.GET('/api/v1/progress', {
			params: { query: { ids: chunk.join(',') } }
		});
		for (const p of data ?? []) map.set(p.video_id, p);
	}
	return map;
}
