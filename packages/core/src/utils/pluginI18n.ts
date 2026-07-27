// Resolve a plugin's localizable manifest string. A field may be `%key%`, which
// resolves against the manifest's per-locale `i18n` tables:
//   i18n[locale][key]  ->  i18n['en-US'][key]  ->  the literal (key unwrapped)
// Manifest validation guarantees every `%key%` has an en-US fallback, so a
// `%key%` always resolves to something. A non-`%key%` string is returned as-is.

const FALLBACK_LOCALE = 'en-US';

/** Grammar mirrors the backend `i18n_key`: `%` + `[A-Za-z0-9_.]+` + `%`. */
const KEY_RE = /^%([A-Za-z0-9_.]+)%$/;

export type PluginI18n = Record<string, Record<string, string>>;

export function resolvePluginI18n(
  value: string | undefined | null,
  i18n: PluginI18n | undefined,
  locale: string,
): string {
  if (!value) return value ?? '';
  const m = KEY_RE.exec(value);
  if (!m) return value;
  const key = m[1];
  const fromLocale = i18n?.[locale]?.[key];
  if (fromLocale) return fromLocale;
  const fromFallback = i18n?.[FALLBACK_LOCALE]?.[key];
  if (fromFallback) return fromFallback;
  // No table (or a locale-switch gap) — show the bare key, not the raw `%key%`.
  return key;
}
