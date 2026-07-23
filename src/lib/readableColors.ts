/**
 * readableColors.ts — strip inline text colors that are unreadable against
 * the app's white-based mail canvas.
 *
 * Mail is rendered on a white background (the dark theme renders the same
 * white-authored document through an invert filter, so "readable on white"
 * is the invariant for both themes). Content pasted from dark-themed apps
 * carries near-white inline colors that assume a dark background the mail
 * doesn't bring along — white-on-white in light theme, dark-on-dark after
 * inversion. This walks inline styles and removes a `color` with hopeless
 * contrast against white, but only when neither the element nor any ancestor
 * supplies its own background: real light-on-dark mail inlines its
 * backgrounds precisely because mail clients strip everything else.
 */

/** Relative luminance above which text is considered unreadable on white
 * (≈ WCAG contrast below 2:1). Light grays used for legitimate muted text
 * (#999 ≈ 0.32) stay below the cutoff. */
const LUMINANCE_CUTOFF = 0.475;

/** Named CSS colors that are near-white; anything here is above the cutoff. */
const LIGHT_NAMED = new Set([
  'white', 'snow', 'ivory', 'mintcream', 'azure', 'aliceblue', 'ghostwhite',
  'whitesmoke', 'seashell', 'beige', 'oldlace', 'floralwhite', 'lightyellow',
  'lightcyan', 'honeydew', 'lavenderblush', 'linen', 'cornsilk',
  'lemonchiffon', 'papayawhip', 'gainsboro', 'lightgray', 'lightgrey',
]);

function channelToLinear(c: number): number {
  const s = c / 255;
  return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

/** Relative luminance of a CSS color as it renders on a white background,
 * or null when the value isn't a form we can judge (those stay untouched).
 * Semi/fully transparent colors return null too — `color: transparent` is a
 * deliberate hiding technique (preheader text), not a contrast accident. */
function luminanceOnWhite(value: string): number | null {
  const v = value.trim().toLowerCase();
  if (LIGHT_NAMED.has(v)) return 1;

  let r: number, g: number, b: number;
  let a = 1;

  const rgbMatch = v.match(/^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:[\s,/]+([\d.]+))?\s*\)$/);
  const hexMatch = v.match(/^#([0-9a-f]{3,8})$/);
  if (rgbMatch) {
    r = parseFloat(rgbMatch[1]);
    g = parseFloat(rgbMatch[2]);
    b = parseFloat(rgbMatch[3]);
    if (rgbMatch[4] !== undefined) a = parseFloat(rgbMatch[4]);
  } else if (hexMatch) {
    const h = hexMatch[1];
    if (h.length === 3 || h.length === 4) {
      r = parseInt(h[0] + h[0], 16);
      g = parseInt(h[1] + h[1], 16);
      b = parseInt(h[2] + h[2], 16);
      if (h.length === 4) a = parseInt(h[3] + h[3], 16) / 255;
    } else if (h.length === 6 || h.length === 8) {
      r = parseInt(h.slice(0, 2), 16);
      g = parseInt(h.slice(2, 4), 16);
      b = parseInt(h.slice(4, 6), 16);
      if (h.length === 8) a = parseInt(h.slice(6, 8), 16) / 255;
    } else {
      return null;
    }
  } else {
    return null;
  }

  if (!Number.isFinite(r) || !Number.isFinite(g) || !Number.isFinite(b)) return null;
  if (a < 0.1) return null;

  // Composite on white, then WCAG relative luminance.
  r = a * r + (1 - a) * 255;
  g = a * g + (1 - a) * 255;
  b = a * b + (1 - a) * 255;
  return 0.2126 * channelToLinear(r) + 0.7152 * channelToLinear(g) + 0.0722 * channelToLinear(b);
}

/** True when the element or an ancestor declares its own background (inline
 * style or legacy bgcolor attribute) — the author designed against that
 * background, so its text colors are not ours to judge. */
function hasOwnBackground(el: HTMLElement, stopAt: HTMLElement | null): boolean {
  for (let n: HTMLElement | null = el; n && n !== stopAt; n = n.parentElement) {
    if (n.getAttribute('bgcolor')) return true;
    const bg = n.style?.backgroundColor || n.style?.background;
    if (bg && bg !== 'transparent' && bg !== 'none' && !bg.startsWith('rgba(0, 0, 0, 0)')) return true;
  }
  return false;
}

/** Remove inline text colors that would be unreadable on the mail canvas.
 * Returns the input unchanged when nothing needed fixing. */
export function ensureReadableTextColors(html: string): string {
  if (!/color/i.test(html)) return html;

  const doc = new DOMParser().parseFromString(html, 'text/html');
  let changed = false;

  for (const el of Array.from(doc.body.querySelectorAll<HTMLElement>('[style*="color"], font[color]'))) {
    const styleColor = el.style?.color;
    const fontColor = el.tagName === 'FONT' ? el.getAttribute('color') : null;
    const value = styleColor || fontColor;
    if (!value) continue;

    const lum = luminanceOnWhite(value);
    if (lum === null || lum <= LUMINANCE_CUTOFF) continue;
    if (hasOwnBackground(el, doc.documentElement)) continue;

    if (styleColor) {
      el.style.removeProperty('color');
      el.style.removeProperty('-webkit-text-fill-color');
      if (!el.getAttribute('style')?.trim()) el.removeAttribute('style');
    }
    if (fontColor) el.removeAttribute('color');
    changed = true;
  }

  if (!changed) return html;
  // The parser hoists leading <style> blocks into <head> — carry them along.
  const headStyles = Array.from(doc.head.querySelectorAll('style')).map((s) => s.outerHTML).join('');
  return headStyles + doc.body.innerHTML;
}
