<script lang="ts">
	// Small inline icon set (Lucide-style strokes) so the app has no icon font dependency.
	const paths = {
		home: 'M3 10.5 12 3l9 7.5V21h-6v-6H9v6H3z',
		search: 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zm10 17-4.35-4.35',
		list: 'M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01',
		subs: 'M4 4h16v12H4zM8 20h8M12 16v4',
		settings:
			'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm7.4-3a7.4 7.4 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7.6 7.6 0 0 0-2-1.2L14.5 3h-5l-.4 2.6a7.6 7.6 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6a7.4 7.4 0 0 0 0 2.4l-2 1.6 2 3.4 2.4-1a7.6 7.6 0 0 0 2 1.2l.4 2.6h5l.4-2.6a7.6 7.6 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6c.1-.4.1-.8.1-1.2z',
		shield: 'M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z',
		key: 'M15 7a4 4 0 1 1-3.9 5H3v3h3v3h3v-3h2.1A4 4 0 0 1 15 7zm1 3h.01',
		logout: 'M15 4h4v16h-4M10 8l-4 4 4 4M6 12h11',
		trash: 'M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3',
		plus: 'M12 5v14M5 12h14',
		chevronUp: 'M6 15l6-6 6 6'
	} as const;

	let {
		name,
		size = 20,
		class: cls = ''
	}: { name: keyof typeof paths; size?: number; class?: string } = $props();
</script>

<svg
	width={size}
	height={size}
	viewBox="0 0 24 24"
	fill="none"
	stroke="currentColor"
	stroke-width="1.8"
	stroke-linecap="round"
	stroke-linejoin="round"
	aria-hidden="true"
	class={cls}><path d={paths[name]} /></svg
>
