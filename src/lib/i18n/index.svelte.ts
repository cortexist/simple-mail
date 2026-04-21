/**
 * Lightweight i18n module for the Mail app.
 *
 * Usage:
 *   import { t, setLocale, locale } from '$lib/i18n';
 *
 *   t('compose.send')          → "Send"
 *   t('titleBar.unread', { count: 5 }) → "5 unread"
 *
 * Adding a new language:
 *   1. Create  src/lib/i18n/xx.json  (copy en.json, translate values)
 *   2. Import it in LOCALES below
 *   3. Add the entry to LANGUAGE_NAMES
 */

import de from './de.json';
import en from './en.json';
import es from './es.json';
import fr from './fr.json';
import it from './it.json';
import ja from './ja.json';
import ko from './ko.json';
import zhCN from './zh-CN.json';
import zhTW from './zh-TW.json';

// ── Available locales ───────────────────────────────────

type LocaleData = Record<string, Record<string, string>>;

const LOCALES: Record<string, LocaleData> = {
  de,
  en,
  es,
  fr,
  it,
  ja,
  ko,
  'zh-CN': zhCN,
  'zh-TW': zhTW,
};

/** Human-readable names shown in the settings picker. */
export const LANGUAGE_NAMES: Record<string, string> = {
  de:      'Deutsch',
  en:      'English',
  es:      'Español',
  fr:      'Français',
  it:      'Italiano',
  ja:      '日本語',
  ko:      '한국어',
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
};

// ── Reactive state ──────────────────────────────────────

let currentLocale = $state('en');

/** The active locale code (reactive). */
export function locale(): string {
  return currentLocale;
}

/** Switch the active locale. */
export function setLocale(code: string) {
  if (LOCALES[code]) {
    currentLocale = code;
  }
}

// ── Translation function ────────────────────────────────

/**
 * Look up a dotted key (e.g. `"compose.send"`) in the active locale,
 * with optional `{placeholder}` interpolation.
 *
 * Falls back to English, then to the raw key if nothing is found.
 */
export function t(key: string, params?: Record<string, string | number>): string {
  const [section, ...rest] = key.split('.');
  const field = rest.join('.');

  const dict = LOCALES[currentLocale] ?? LOCALES.en;
  let value = dict?.[section]?.[field];

  // Fallback to English if the key is missing in the current locale
  if (value === undefined && currentLocale !== 'en') {
    value = LOCALES.en?.[section]?.[field];
  }

  // If still not found, return the raw key so missing translations are obvious
  if (value === undefined) return key;

  // Interpolate {placeholders}
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      value = value.replaceAll(`{${k}}`, String(v));
    }
  }

  return value;
}

/**
 * Detect the user's preferred language from the browser.
 * Returns the best matching locale code, or 'en' as fallback.
 */
export function detectLocale(): string {
  if (typeof navigator === 'undefined') return 'en';
  const lang = navigator.language ?? 'en';

  // Chinese needs the region to distinguish Simplified vs Traditional
  if (lang.startsWith('zh')) {
    const code = (lang.startsWith('zh-TW') || lang.startsWith('zh-HK') || lang.startsWith('zh-Hant'))
      ? 'zh-TW'
      : 'zh-CN';
    return LOCALES[code] ? code : 'en';
  }

  // All other languages: use base code only
  const base = lang.split('-')[0];
  return LOCALES[base] ? base : 'en';
}
