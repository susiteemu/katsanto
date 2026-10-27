<script lang="ts">
	import { locale, m } from '#lib/i18n/index.js';
	import {
		LANGUAGE_CODES,
		POSITIONS,
		saveSubtitleSettings,
		subtitleSettings,
		type SubtitleSettings
	} from '#lib/subtitles.svelte.js';
	import Card from './Card.svelte';

	const s = $derived(subtitleSettings());
	const t = $derived(m.subtitles);

	const names = $derived(new Intl.DisplayNames([locale.current], { type: 'language' }));
	const nameOf = (code: string) => names.of(code) ?? code;
	/** Languages not yet preferred, by name. */
	const addable = $derived(
		LANGUAGE_CODES.filter((c) => !s.preferred.includes(c))
			.map((code) => ({ code, name: nameOf(code) }))
			.sort((a, b) => a.name.localeCompare(b.name, locale.current))
	);

	function update(fn: (s: SubtitleSettings) => void) {
		const next = $state.snapshot(s) as SubtitleSettings;
		fn(next);
		saveSubtitleSettings(next);
	}

	function moveUp(i: number) {
		update((n) => n.preferred.splice(i - 1, 2, n.preferred[i], n.preferred[i - 1]));
	}
</script>

<Card title={t.title} description={t.hint}>
	<div class="flex flex-col gap-6">
		<fieldset>
			<legend class="mb-2 text-sm font-medium">{t.position}</legend>
			<div class="flex flex-wrap gap-2">
				{#each POSITIONS as p (p)}
					<label
						class="cursor-pointer rounded-card border border-border px-3 py-2 text-sm has-checked:border-accent has-checked:bg-accent/10 has-focus-visible:outline-2 has-focus-visible:outline-accent"
					>
						<input
							type="radio"
							name="subtitlePosition"
							class="sr-only"
							checked={s.position === p}
							onchange={() => update((n) => (n.position = p))}
						/>
						{t.positions[p]}
					</label>
				{/each}
			</div>
		</fieldset>

		<fieldset>
			<legend class="text-sm font-medium">{t.preferred}</legend>
			<p class="mb-2 text-sm text-muted">{t.preferredHint}</p>
			{#if s.preferred.length}
				<ol class="mb-3 flex flex-wrap gap-2">
					{#each s.preferred as code, i (code)}
						<li
							class="flex items-center gap-1 rounded-card border border-border py-1 pr-1 pl-3 text-sm"
						>
							<span lang={code}>{nameOf(code)}</span>
							{#if i > 0}
								<button
									type="button"
									class="grid size-7 place-items-center rounded-card hover:bg-surface-2"
									aria-label={t.moveUp(nameOf(code))}
									title={t.moveUp(nameOf(code))}
									onclick={() => moveUp(i)}>↑</button
								>
							{/if}
							<button
								type="button"
								class="grid size-7 place-items-center rounded-card hover:bg-surface-2"
								aria-label={t.removeLanguage(nameOf(code))}
								title={t.removeLanguage(nameOf(code))}
								onclick={() => update((n) => n.preferred.splice(i, 1))}>✕</button
							>
						</li>
					{/each}
				</ol>
			{/if}
			<select
				aria-label={t.addLanguage}
				class="h-10 rounded-card border border-border bg-surface px-3"
				onchange={(e) => {
					const code = e.currentTarget.value;
					e.currentTarget.value = '';
					if (code) update((n) => n.preferred.push(code));
				}}
			>
				<option value="">{t.addLanguage}</option>
				{#each addable as l (l.code)}<option value={l.code}>{l.name}</option>{/each}
			</select>
		</fieldset>

		<label class="flex items-start gap-2">
			<input
				type="checkbox"
				class="mt-1 size-4 accent-[var(--accent)]"
				checked={s.latinOnly}
				onchange={(e) => update((n) => (n.latinOnly = e.currentTarget.checked))}
			/>
			<span>
				{t.latinOnly}
				<span class="block text-sm text-muted">{t.latinOnlyHint}</span>
			</span>
		</label>
	</div>
</Card>
