<script lang="ts">
	import { m } from '#lib/i18n/index.js';
	import {
		ACCENTS,
		DEFAULTS,
		THEMES,
		appearance,
		normalize,
		type Settings
	} from '#lib/appearance.svelte.js';
	import { session } from '#lib/session.svelte.js';
	import Alert from './Alert.svelte';
	import Button from './Button.svelte';
	import Card from './Card.svelte';

	const s = $derived(appearance.settings);
	let css = $state(appearance.settings.customCss);
	let importError = $state('');
	let fileInput: HTMLInputElement;

	const set = (patch: Partial<Settings>) => appearance.update(patch, session.me?.settings ?? {});

	type Option<T> = { id: T; label: string };
	/** Options from a map of labels, in the map's order. */
	const options = <T extends string>(labels: Record<T, string>): Option<T>[] =>
		(Object.keys(labels) as T[]).map((id) => ({ id, label: labels[id] }));

	const a = $derived(m.appearance);
	const THEME_OPTIONS = $derived(THEMES.map((t) => ({ id: t.id, label: a.themes[t.id] })));
	const RADII = $derived(options(a.radii));
	const FONTS = $derived(options(a.fonts));
	const DENSITIES = $derived(options(a.densities));
	const SIZES = $derived(options(a.sizes));
	const LAYOUTS = $derived(options(a.layouts));
	const DETAILS = $derived(options(a.details));

	function exportTheme() {
		const json = JSON.stringify(
			{ katsanto: 'appearance', ...$state.snapshot(appearance.settings) },
			null,
			2
		);
		const url = URL.createObjectURL(new Blob([json], { type: 'application/json' }));
		Object.assign(document.createElement('a'), {
			href: url,
			download: 'katsanto-theme.json'
		}).click();
		URL.revokeObjectURL(url);
	}

	async function importTheme(e: Event) {
		importError = '';
		const file = (e.currentTarget as HTMLInputElement).files?.[0];
		if (!file) return;
		try {
			const data = JSON.parse(await file.text());
			if (data?.katsanto !== 'appearance') throw new Error();
			const settings = normalize(data);
			set(settings);
			css = settings.customCss;
		} catch {
			importError = m.appearance.badFile;
		} finally {
			fileInput.value = '';
		}
	}

	function reset() {
		if (!confirm(m.appearance.confirmReset)) return;
		set(DEFAULTS);
		css = '';
	}
</script>

{#snippet choice<T extends string>(
	legend: string,
	name: string,
	options: Option<T>[],
	value: T,
	onpick: (v: T) => void
)}
	<fieldset>
		<legend class="mb-2 text-sm font-medium">{legend}</legend>
		<div class="flex flex-wrap gap-2">
			{#each options as o (o.id)}
				<label
					class="cursor-pointer rounded-card border border-border px-3 py-2 text-sm has-checked:border-accent has-checked:bg-accent/10 has-focus-visible:outline-2 has-focus-visible:outline-accent"
				>
					<input
						type="radio"
						{name}
						class="sr-only"
						checked={value === o.id}
						onchange={() => onpick(o.id)}
					/>
					{o.label}
				</label>
			{/each}
		</div>
	</fieldset>
{/snippet}

<Card title={a.title} description={a.hint}>
	<div class="flex flex-col gap-6">
		{@render choice(a.theme, 'theme', THEME_OPTIONS, s.theme, (theme) => set({ theme }))}

		<fieldset>
			<legend class="mb-2 text-sm font-medium">{a.accent}</legend>
			<div class="flex flex-wrap items-center gap-2">
				{#each ACCENTS as preset (preset.hue)}
					<label
						class="grid size-9 cursor-pointer place-items-center rounded-full ring-offset-2 ring-offset-surface has-checked:ring-2 has-checked:ring-fg has-focus-visible:ring-2 has-focus-visible:ring-accent"
						style="background: oklch(0.62 0.19 {preset.hue})"
						title={a.accents[preset.hue]}
					>
						<input
							type="radio"
							name="accent"
							class="sr-only"
							aria-label={a.accents[preset.hue]}
							checked={!s.accentColor && s.accentHue === preset.hue}
							onchange={() => set({ accentHue: preset.hue, accentColor: null })}
						/>
					</label>
				{/each}
				<label
					class="relative grid size-9 cursor-pointer place-items-center overflow-hidden rounded-full border border-border text-xs ring-offset-2 ring-offset-surface {s.accentColor
						? 'ring-2 ring-fg'
						: ''}"
					title={a.customColor}
					style={s.accentColor ? `background: ${s.accentColor}` : ''}
				>
					{#if !s.accentColor}<span aria-hidden="true">+</span>{/if}
					<input
						type="color"
						aria-label={a.customAccent}
						value={s.accentColor ?? '#00878c'}
						oninput={(e) => set({ accentColor: e.currentTarget.value })}
						class="absolute inset-0 cursor-pointer opacity-0"
					/>
				</label>
			</div>
		</fieldset>

		<div class="grid gap-6 lg:grid-cols-2">
			{@render choice(a.corners, 'radius', RADII, s.radius, (radius) => set({ radius }))}
			{@render choice(a.font, 'font', FONTS, s.font, (font) => set({ font }))}
			{@render choice(a.density, 'density', DENSITIES, s.density, (density) => set({ density }))}
			{@render choice(a.layout, 'layout', LAYOUTS, s.layout, (layout) => set({ layout }))}
			{@render choice(a.cardSize, 'cardSize', SIZES, s.cardSize, (cardSize) => set({ cardSize }))}
		</div>

		<fieldset>
			<legend class="mb-2 text-sm font-medium">{a.show}</legend>
			<div class="flex flex-wrap gap-x-5 gap-y-2">
				{#each DETAILS as d (d.id)}
					<label class="flex items-center gap-2 text-sm">
						<input
							type="checkbox"
							class="size-4 accent-[var(--accent)]"
							checked={s.show[d.id]}
							onchange={(e) => set({ show: { ...s.show, [d.id]: e.currentTarget.checked } })}
						/>
						{d.label}
					</label>
				{/each}
			</div>
		</fieldset>

		<label class="flex items-start gap-2 text-sm">
			<input
				type="checkbox"
				class="mt-0.5 size-4 shrink-0 accent-[var(--accent)]"
				checked={s.groupChannels}
				onchange={(e) => set({ groupChannels: e.currentTarget.checked })}
			/>
			<span>
				<span class="block font-medium">{a.groupChannels}</span>
				<span class="block text-muted">{a.groupChannelsHint}</span>
			</span>
		</label>

		<details class="rounded-card border border-border p-4">
			<summary class="cursor-pointer text-sm font-medium">{a.customCss}</summary>
			<p class="mt-2 text-sm text-muted">
				{a.customCssHint} <code>:root {'{'} --bg: #101820; {'}'}</code>. {a.customCssOnlyYou}
			</p>
			<textarea
				bind:value={css}
				rows="6"
				spellcheck="false"
				aria-label={a.customCss}
				maxlength={20000}
				class="mt-3 w-full rounded-card border border-border bg-surface p-3 font-mono text-sm"
			></textarea>
			<div class="mt-2 flex gap-2">
				<Button size="sm" onclick={() => set({ customCss: css })}>{a.applyCss}</Button>
				{#if s.customCss}
					<Button size="sm" variant="ghost" onclick={() => ((css = ''), set({ customCss: '' }))}
						>{m.common.remove}</Button
					>
				{/if}
			</div>
		</details>

		{#if importError}<Alert>{importError}</Alert>{/if}
		<div class="flex flex-wrap gap-2 border-t border-border pt-4">
			<Button size="sm" onclick={exportTheme}>{a.export}</Button>
			<Button size="sm" onclick={() => fileInput.click()}>{a.import}</Button>
			<input
				bind:this={fileInput}
				type="file"
				accept="application/json,.json"
				class="hidden"
				onchange={importTheme}
				aria-label={a.themeFile}
			/>
			<Button size="sm" variant="ghost" onclick={reset}>{a.reset}</Button>
		</div>
	</div>
</Card>
