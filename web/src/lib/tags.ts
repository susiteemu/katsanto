/** Tag colors offered in the UI, as OKLCH hues (null: neutral gray). */
export const TAG_COLORS: { id: string; label: string; hue: number | null }[] = [
	{ id: '', label: 'Default', hue: null },
	{ id: 'red', label: 'Red', hue: 25 },
	{ id: 'orange', label: 'Orange', hue: 55 },
	{ id: 'amber', label: 'Amber', hue: 85 },
	{ id: 'green', label: 'Green', hue: 145 },
	{ id: 'teal', label: 'Teal', hue: 190 },
	{ id: 'blue', label: 'Blue', hue: 250 },
	{ id: 'violet', label: 'Violet', hue: 290 },
	{ id: 'pink', label: 'Pink', hue: 350 },
	{ id: 'gray', label: 'Gray', hue: null }
];

/** CSS color for a tag color id. */
export function tagColor(id: string): string {
	const hue = TAG_COLORS.find((c) => c.id === id)?.hue;
	return hue == null ? 'var(--muted)' : `oklch(0.62 0.17 ${hue})`;
}
