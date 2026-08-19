/**
 * theme.ts — the accent palette.
 *
 * An accent preset is one color, written as the two OKLCH channels that
 * distinguish it: a hue and how bright that hue is at full strength. Every
 * shade the UI reaches for — the hover and pressed steps, the legible-on-page
 * variant, the muted surface, the selection wash, the foreground that sits on
 * an accent fill — is derived from those two numbers by the accent block at
 * the top of `app.css`. A preset never spells out a shade, the light and dark
 * themes can never drift apart, and adding a color is one line here.
 *
 * OKLCH is what makes two numbers enough. Its lightness axis is perceptual, so
 * `lightness` means the same thing at every hue: a fixed value reads equally
 * bright whatever the hue, and the contrast decisions downstream can be made
 * from that one number instead of per color.
 *
 * The reason it is not one number is that hues are not equally bright at full
 * strength — a gold that sits as dark as a blue is olive, not gold. Lightness
 * is the "how vivid is this color" half of the choice; app.css takes it from
 * there, dimming the hue where it has to carry text.
 */

export interface AccentPreset {
  /** Persisted in the settings store — renaming one resets that user's accent. */
  id: string;
  /** OKLCH hue angle in degrees. */
  hue: number;
  /**
   * OKLCH lightness of the solid fill in the light theme, 0–1. The dark theme
   * raises anything below its floor; nothing else needs a per-preset value.
   */
  lightness: number;
  /** Key under `settings.` in the locale files. */
  labelKey: string;
}

/**
 * Nine hues, roughly evenly spaced around the wheel and laid out as a 3x3 grid
 * in reading order. The five that predate this list keep both their id and
 * their hue, so an existing preference still resolves to the same color.
 *
 * Orange, gold and lime carry a higher lightness because those hues are only
 * themselves when they are bright; the rest sit at 0.55, dark enough to take
 * white text — the deepest of the nine still clears 4.5:1 both as a fill under
 * white text and as lettering on the page. That split is the one the foreground
 * rule keys off.
 */
export const ACCENT_PRESETS: readonly AccentPreset[] = [
  { id: 'red',     hue:  25, lightness: 0.51, labelKey: 'accentRed' },
  { id: 'orange',  hue:  60, lightness: 0.70, labelKey: 'accentOrange' },
  { id: 'gold',    hue:  93, lightness: 0.80, labelKey: 'accentGold' },
  { id: 'lime',    hue: 128, lightness: 0.75, labelKey: 'accentLime' },
  { id: 'green',   hue: 166, lightness: 0.51, labelKey: 'accentGreen' },
  { id: 'teal',    hue: 205, lightness: 0.51, labelKey: 'accentTeal' },
  { id: 'blue',    hue: 251, lightness: 0.51, labelKey: 'accentBlue' },
  { id: 'purple',  hue: 281, lightness: 0.51, labelKey: 'accentPurple' },
  { id: 'magenta', hue: 346, lightness: 0.51, labelKey: 'accentMagenta' },
];

export const DEFAULT_ACCENT_ID = 'blue';

const DEFAULT_PRESET = ACCENT_PRESETS.find((p) => p.id === DEFAULT_ACCENT_ID)!;

/** The preset for a stored id, falling back to the default for an unknown one. */
export function accentPreset(id: string): AccentPreset {
  return ACCENT_PRESETS.find((p) => p.id === id) ?? DEFAULT_PRESET;
}

/**
 * Color for a swatch previewing `preset`. It borrows the live `--accent-c` and
 * the theme's lightness floor, so every swatch shows the shade that preset
 * would actually take in the theme on screen rather than a fixed sample.
 */
export function accentSwatch(preset: AccentPreset): string {
  return `oklch(max(${preset.lightness}, var(--accent-l-floor)) var(--accent-c) ${preset.hue})`;
}

/** Point the whole app's accent at `id`. The derivation lives in app.css. */
export function applyAccent(id: string): void {
  const { hue, lightness } = accentPreset(id);
  const style = document.documentElement.style;
  style.setProperty('--accent-h', String(hue));
  style.setProperty('--accent-base-l', String(lightness));
}
