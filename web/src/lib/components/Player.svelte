<script lang="ts">
	import '@videojs/html/video/player';
	import '@videojs/html/video/skin';
	import '@videojs/html/media/dash-video';
	import '@videojs/html/ui/hotkey';
	import { onMount } from 'svelte';
	import type { Schemas } from '#lib/api/client.js';
	import type { Category, SegmentAction } from '#lib/community.svelte.js';
	import { trackProgress, type MediaLike } from '#lib/progress.js';
	import SegmentSkipper from './SegmentSkipper.svelte';

	/**
	 * The skin's shortcuts, made page-wide (the skin's own work only while the player has
	 * focus). Up/Down stay player-only so they still scroll the page. Listed in ShortcutsDialog.
	 */
	const HOTKEYS: { keys: string; action: string; value?: number }[] = [
		{ keys: 'Space', action: 'togglePaused' },
		{ keys: 'k', action: 'togglePaused' },
		{ keys: 'm', action: 'toggleMuted' },
		{ keys: 'ArrowRight', action: 'seekStep' },
		{ keys: 'ArrowLeft', action: 'seekStep' },
		{ keys: 'l', action: 'seekStep' },
		{ keys: 'j', action: 'seekStep' },
		{ keys: '0-9', action: 'seekToPercent' },
		{ keys: '>', action: 'speedUp' },
		{ keys: '<', action: 'speedDown' },
		{ keys: 'f', action: 'toggleFullscreen' },
		{ keys: 'c', action: 'toggleSubtitles' },
		{ keys: 'i', action: 'togglePictureInPicture' }
	];

	interface Props {
		videoId: string;
		/** DASH manifest URL. */
		src: string;
		title: string;
		channel?: string;
		poster?: string | null;
		captions?: Schemas['CaptionTrack'][];
		chapters?: Schemas['Chapter'][];
		duration?: number;
		/** Position to start from (seconds). */
		startAt?: number;
		autoplay?: boolean;
		/** Called when playback reaches the end (play-list mode). */
		onended?: () => void;
		/** Next / previous video (play-list mode): media keys, Shift+N / Shift+P. */
		onnext?: () => void;
		onprevious?: () => void;
		/** SponsorBlock segments and what to do with each category. */
		segments?: Schemas['Segment'][];
		segmentActions?: Record<Category, SegmentAction>;
	}

	let {
		videoId,
		src,
		title,
		channel = '',
		poster = null,
		captions = [],
		chapters = [],
		duration = 0,
		startAt = 0,
		autoplay = true,
		onended,
		onnext,
		onprevious,
		segments = [],
		segmentActions
	}: Props = $props();

	let skinEl = $state<HTMLElement>();

	/**
	 * Opus audio where the browser plays it: YouTube's AAC streams start ~36 ms late
	 * (encoder priming without an edit list), Opus lines up with the picture.
	 */
	const manifestUrl = $derived.by(() => {
		const url = new URL(src, location.href);
		const opus =
			typeof MediaSource !== 'undefined' &&
			MediaSource.isTypeSupported('audio/webm; codecs="opus"');
		url.searchParams.set('audio', opus ? 'opus' : 'aac');
		return url.href;
	});

	/** The `<dash-video>` element: a media element as far as Katsanto is concerned. */
	type Media = MediaLike & Pick<HTMLMediaElement, 'pause' | 'playbackRate'>;
	let media: Media;

	/** Jump to a time (description timestamps, chapters) and keep playing. */
	export function seek(seconds: number) {
		media.currentTime = seconds;
		void media.play().catch(() => {});
	}

	/** The time slider shows chapters from a WebVTT chapters track. */
	const chaptersUrl = $derived.by(() => {
		if (chapters.length === 0) return null;
		const ts = (s: number) => new Date(s * 1000).toISOString().slice(11, 23);
		const cues = chapters.map((c, i) => {
			const end = chapters[i + 1]?.start_s ?? Math.max(duration, c.start_s + 1);
			return `${ts(c.start_s)} --> ${ts(end)}\n${c.title}`;
		});
		return URL.createObjectURL(
			new Blob([`WEBVTT\n\n${cues.join('\n\n')}\n`], { type: 'text/vtt' })
		);
	});
	$effect(() => {
		const url = chaptersUrl;
		return () => {
			if (url) URL.revokeObjectURL(url);
		};
	});

	/** Lock screen, notification and hardware media key controls. */
	function mediaSession(): () => void {
		if (!('mediaSession' in navigator)) return () => {};
		const ms = navigator.mediaSession;
		ms.metadata = new MediaMetadata({
			title,
			artist: channel,
			artwork: poster ? [{ src: new URL(poster, location.href).href, sizes: '1280x720' }] : []
		});
		const skip = (by: number) => {
			media.currentTime = Math.min(Math.max(media.currentTime + by, 0), media.duration || Infinity);
		};
		const handlers: [MediaSessionAction, MediaSessionActionHandler | null][] = [
			['play', () => void media.play().catch(() => {})],
			['pause', () => media.pause()],
			['seekbackward', (d) => skip(-(d.seekOffset ?? 10))],
			['seekforward', (d) => skip(d.seekOffset ?? 10)],
			['seekto', (d) => d.seekTime != null && (media.currentTime = d.seekTime)]
		];
		for (const [action, handler] of handlers) {
			try {
				ms.setActionHandler(action, handler);
			} catch {
				// Unsupported action in this browser.
			}
		}
		// Keeps the lock screen's progress bar in sync.
		const position = () => {
			if (!Number.isFinite(media.duration) || media.duration <= 0) return;
			try {
				ms.setPositionState({
					duration: media.duration,
					position: Math.min(media.currentTime, media.duration),
					playbackRate: media.playbackRate || 1
				});
			} catch {
				// Ignore transient invalid states.
			}
		};
		const events = ['loadedmetadata', 'seeked', 'ratechange', 'play', 'pause'];
		for (const e of events) media.addEventListener(e, position);
		return () => {
			for (const e of events) media.removeEventListener(e, position);
			for (const [action] of handlers) {
				try {
					ms.setActionHandler(action, null);
				} catch {
					// Unsupported.
				}
			}
			ms.metadata = null;
		};
	}

	// The list (and so next/previous) can arrive after the player has started.
	$effect(() => {
		if (!('mediaSession' in navigator)) return;
		const tracks: [MediaSessionAction, (() => void) | undefined][] = [
			['previoustrack', onprevious],
			['nexttrack', onnext]
		];
		for (const [action, fn] of tracks) {
			try {
				navigator.mediaSession.setActionHandler(action, fn ? () => fn() : null);
			} catch {
				// Unsupported.
			}
		}
		return () => {
			for (const [action] of tracks) {
				try {
					navigator.mediaSession.setActionHandler(action, null);
				} catch {
					// Unsupported.
				}
			}
		};
	});

	onMount(() => {
		// Resume once the duration is known; seeking earlier is ignored by some engines.
		const resume = () => {
			if (startAt > 0 && startAt < media.duration - 5) media.currentTime = startAt;
		};
		if (media.readyState >= 1) resume();
		else media.addEventListener('loadedmetadata', resume, { once: true });

		const stopSession = mediaSession();
		const stop = trackProgress(media, videoId);
		const ended = () => onended?.();
		media.addEventListener('ended', ended);
		return () => {
			stop();
			stopSession();
			media.removeEventListener('ended', ended);
			media.removeEventListener('loadedmetadata', resume);
		};
	});
</script>

<video-player class="player">
	<video-skin class="skin" bind:this={skinEl}>
		<dash-video
			bind:this={media}
			source={{
				// Absolute: dash.js's request plumbing (CMCD) can't parse relative URLs.
				src: manifestUrl,
				// CMCD sends playback telemetry along with segment requests; Katsanto doesn't want it.
				engine: { dashJs: { streaming: { cmcd: { enabled: false } } } }
			}}
			poster={poster ?? undefined}
			{autoplay}
			playsinline
			crossorigin="anonymous"
		>
			{#each captions as c (c.id)}
				<track kind="subtitles" src={c.url} srclang={c.lang} label={c.label} />
			{/each}
			{#if chaptersUrl}
				<track kind="chapters" src={chaptersUrl} default />
			{/if}
		</dash-video>
		{#each HOTKEYS as h (h.keys)}
			<media-hotkey target="document" keys={h.keys} action={h.action} value={h.value}
			></media-hotkey>
		{/each}
		<!-- Inside the skin, so it is slotted into the player and stays visible in fullscreen. -->
		{#if segments.length && segmentActions && skinEl}
			<SegmentSkipper media={() => media} skin={() => skinEl} {segments} actions={segmentActions} />
		{/if}
	</video-skin>
</video-player>

<style>
	.player {
		display: block;
		/* Follow Katsanto's theme. */
		--media-accent: var(--accent);
		--media-border-radius: var(--radius);
	}
	.skin {
		display: block;
		width: 100%;
		aspect-ratio: 16 / 9;
		background: black;
		border-radius: var(--radius);
		overflow: hidden;
	}
	@media (max-width: 767px) {
		/* Edge-to-edge on phones. */
		.player {
			--media-border-radius: 0;
		}
		.skin {
			border-radius: 0;
		}
	}
</style>
