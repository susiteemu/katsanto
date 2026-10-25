<script lang="ts">
	/** The Katsanto mark: a retro TV on an accent tile, in the user's accent color. */
	let { class: className = 'size-8' }: { class?: string } = $props();
</script>

<svg viewBox="0 0 32 32" class={className} aria-hidden="true">
	<rect width="32" height="32" rx="8" class="fill-accent" />
	<path
		d="M11.5 5.5 16 10.5l4.5-5M9 27.5v-1.6M23 27.5v-1.6"
		fill="none"
		stroke-width="2"
		stroke-linecap="round"
		stroke-linejoin="round"
		class="stroke-accent-fg"
	/>
	<rect x="5" y="10.5" width="22" height="15" rx="3.5" class="fill-accent-fg" />
	<rect x="7.8" y="13.2" width="12.8" height="9.6" rx="2.2" class="fill-accent" />
	<circle cx="23.6" cy="15.2" r="1.3" class="fill-accent" />
	<circle cx="23.6" cy="20" r="1.3" class="fill-accent" />
</svg>
