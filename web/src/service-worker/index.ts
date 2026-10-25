// An offline shell: the app's own files are cached so Katsanto opens (and says it's
// offline) without a connection. API responses and video are never cached.

import { version } from '$app/env';
import { assets, immutable } from '$app/manifest';
import { self as sw } from '$app/service-worker';

const CACHE = `katsanto-${version}`;
const SHELL = '/index.html';
const ASSETS = new Set([...immutable, ...assets].map((f) => `/${f.path}`).concat(SHELL));

sw.addEventListener('install', (event) => {
	event.waitUntil(
		caches
			.open(CACHE)
			.then((cache) => cache.addAll([...ASSETS]))
			.then(() => sw.skipWaiting())
	);
});

sw.addEventListener('activate', (event) => {
	event.waitUntil(
		caches
			.keys()
			.then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
			.then(() => sw.clients.claim())
	);
});

sw.addEventListener('fetch', (event) => {
	const req = event.request;
	if (req.method !== 'GET') return;
	const url = new URL(req.url);
	if (url.origin !== sw.location.origin) return;
	if (url.pathname.startsWith('/api/') || url.pathname.startsWith('/proxy/')) return;

	if (ASSETS.has(url.pathname)) {
		// Build output is content-hashed (or versioned with this worker): cache first.
		event.respondWith(caches.match(url.pathname).then((hit) => hit ?? fetch(req)));
	} else if (req.mode === 'navigate') {
		// App routes: always the latest shell when online, the cached one when not.
		event.respondWith(
			fetch(req).catch(async () => (await caches.match(SHELL)) ?? Response.error())
		);
	}
});
