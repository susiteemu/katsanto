import { api } from '#lib/api/client.js';

export type Theme = 'system' | 'light' | 'dark' | 'black' | 'contrast';
export type Radius = 'none' | 'small' | 'medium' | 'large';
export type Font = 'system' | 'rounded' | 'serif' | 'mono';
export type Density = 'compact' | 'comfortable' | 'spacious';
export type CardSize = 'small' | 'medium' | 'large';
export type Layout = 'grid' | 'list';

export interface Settings {
	theme: Theme;
	/** OKLCH hue of the accent, used unless `accentColor` is set. */
	accentHue: number;
	/** A custom accent color (`#rrggbb`), overriding the hue. */
	accentColor: string | null;
	radius: Radius;
	font: Font;
	density: Density;
	cardSize: CardSize;
	layout: Layout;
	show: { avatars: boolean; views: boolean; dates: boolean; durations: boolean };
	/** Extra CSS applied after Katsanto's own styles. */
	customCss: string;
}

export const DEFAULTS: Settings = {
	theme: 'system',
	accentHue: 195,
	accentColor: null,
	radius: 'medium',
	font: 'system',
	density: 'comfortable',
	cardSize: 'medium',
	layout: 'grid',
	show: { avatars: true, views: true, dates: true, durations: true },
	customCss: ''
};

export const THEMES: { id: Theme; label: string }[] = [
	{ id: 'system', label: 'System' },
	{ id: 'light', label: 'Light' },
	{ id: 'dark', label: 'Dark' },
	{ id: 'black', label: 'Black (AMOLED)' },
	{ id: 'contrast', label: 'High contrast' }
];

/** Accent presets as OKLCH hues. */
export const ACCENTS: { hue: number; label: string }[] = [
	{ hue: 350, label: 'Raspberry' },
	{ hue: 25, label: 'Red' },
	{ hue: 60, label: 'Orange' },
	{ hue: 145, label: 'Green' },
	{ hue: 195, label: 'Teal' },
	{ hue: 255, label: 'Blue' },
	{ hue: 290, label: 'Violet' }
];

const RADII: Record<Radius, string> = {
	none: '0',
	small: '0.375rem',
	medium: '0.75rem',
	large: '1.25rem'
};
const FONTS: Record<Font, string> = {
	system: "ui-sans-serif, system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif",
	rounded: "ui-rounded, 'SF Pro Rounded', 'Nunito', 'Varela Round', system-ui, sans-serif",
	serif: "ui-serif, 'Iowan Old Style', 'Charter', Georgia, 'Times New Roman', serif",
	mono: "ui-monospace, 'SF Mono', 'Cascadia Code', Menlo, Consolas, monospace"
};
/** Root font size: everything in rem scales with it. */
const DENSITY: Record<Density, string> = {
	compact: '14px',
	comfortable: '16px',
	spacious: '17.5px'
};

const KEY = 'katsanto.appearance';
const MAX_CSS = 20_000;

/** Validate and complete settings from storage, the server or an imported file. */
export function normalize(input: unknown): Settings {
	const s = (input && typeof input === 'object' ? input : {}) as Record<string, unknown>;
	const pick = <T extends string>(v: unknown, allowed: readonly T[], fallback: T): T =>
		allowed.includes(v as T) ? (v as T) : fallback;
	const show = (s.show ?? {}) as Record<string, unknown>;
	const bool = (v: unknown, fallback: boolean) => (typeof v === 'boolean' ? v : fallback);
	// Older versions stored the hue as `accent`.
	const hue =
		typeof s.accentHue === 'number' ? s.accentHue : typeof s.accent === 'number' ? s.accent : null;
	return {
		theme: pick(s.theme, ['system', 'light', 'dark', 'black', 'contrast'], DEFAULTS.theme),
		accentHue: hue !== null && hue >= 0 && hue <= 360 ? hue : DEFAULTS.accentHue,
		accentColor:
			typeof s.accentColor === 'string' && /^#[0-9a-f]{6}$/i.test(s.accentColor)
				? s.accentColor
				: null,
		radius: pick(s.radius, ['none', 'small', 'medium', 'large'], DEFAULTS.radius),
		font: pick(s.font, ['system', 'rounded', 'serif', 'mono'], DEFAULTS.font),
		density: pick(s.density, ['compact', 'comfortable', 'spacious'], DEFAULTS.density),
		cardSize: pick(s.cardSize, ['small', 'medium', 'large'], DEFAULTS.cardSize),
		layout: pick(s.layout, ['grid', 'list'], DEFAULTS.layout),
		show: {
			avatars: bool(show.avatars, true),
			views: bool(show.views, true),
			dates: bool(show.dates, true),
			durations: bool(show.durations, true)
		},
		customCss: typeof s.customCss === 'string' ? s.customCss.slice(0, MAX_CSS) : ''
	};
}

/** Black or white text, whichever reads better on `hex`. */
export function contrastText(hex: string): string {
	const [r, g, b] = [1, 3, 5].map((i) => {
		const c = parseInt(hex.slice(i, i + 2), 16) / 255;
		return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
	});
	const luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
	return luminance > 0.36 ? '#111111' : '#ffffff';
}

/** CSS custom properties for the settings (theme colors come from the stylesheet). */
export function cssVars(s: Settings): Record<string, string> {
	const vars: Record<string, string> = {
		'--accent-hue': String(s.accentHue),
		'--radius': RADII[s.radius],
		'--k-font': FONTS[s.font],
		'--base-size': DENSITY[s.density]
	};
	if (s.accentColor) {
		vars['--accent'] = s.accentColor;
		vars['--accent-fg'] = contrastText(s.accentColor);
	}
	return vars;
}

/**
 * Appearance settings. Saved per user on the server, and mirrored in localStorage
 * (as ready-made CSS variables) so app.html can apply them before the first paint.
 */
class Appearance {
	settings = $state<Settings>(DEFAULTS);
	#saveTimer: ReturnType<typeof setTimeout> | undefined;

	constructor() {
		try {
			this.settings = normalize(JSON.parse(localStorage.getItem(KEY) ?? '{}').settings);
		} catch {
			// Defaults.
		}
	}

	get theme() {
		return this.settings.theme;
	}

	apply() {
		const s = this.settings;
		const root = document.documentElement;
		if (s.theme === 'system') delete root.dataset.theme;
		else root.dataset.theme = s.theme;
		const vars = cssVars(s);
		for (const p of ['--accent', '--accent-fg']) root.style.removeProperty(p);
		for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);

		let style = document.getElementById('katsanto-custom-css');
		if (s.customCss) {
			if (!style) {
				style = document.createElement('style');
				style.id = 'katsanto-custom-css';
			}
			// Last in <head>, so it wins over the app's stylesheets (app.html's pre-paint copy
			// is inserted before them).
			if (style !== document.head.lastElementChild) document.head.append(style);
			// textContent: the CSS is never parsed as HTML.
			style.textContent = s.customCss;
		} else {
			style?.remove();
		}

		try {
			localStorage.setItem(
				KEY,
				JSON.stringify({ settings: s, theme: s.theme, vars, css: s.customCss })
			);
		} catch {
			// Private mode etc.: still applies for this page load.
		}
	}

	/** Adopt the settings stored with the user's account. */
	fromServer(userSettings: Record<string, unknown> | undefined) {
		this.settings = normalize(userSettings?.appearance);
		this.apply();
	}

	/**
	 * Change settings, apply immediately and save to the account (debounced, so
	 * dragging a slider doesn't send a request per step).
	 */
	update(patch: Partial<Settings>, userSettings: Record<string, unknown>) {
		this.settings = normalize({ ...this.settings, ...patch });
		this.apply();
		clearTimeout(this.#saveTimer);
		const snapshot = $state.snapshot(this.settings);
		// The whole settings object is saved, so keep the caller's copy current.
		userSettings.appearance = snapshot;
		this.#saveTimer = setTimeout(() => {
			void api.PUT('/api/v1/me/settings', { body: { ...userSettings } });
		}, 500);
	}
}

export const appearance = new Appearance();
