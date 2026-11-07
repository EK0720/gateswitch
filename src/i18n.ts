// i18n. English is the source language: code and backend messages are written
// in English and are also the dictionary keys. Every other language is a
// translation file keyed by the English text (src/i18n/<lang>.ts); a missing
// entry falls back to English. Placeholders: t("Removed {n}", { n: 2 }).
//
// Adding a language: copy src/i18n/vi.ts to <code>.ts, translate the values,
// register it in `languages` below.
import { vi } from "./i18n/vi";
import { viPatterns } from "./i18n/vi-patterns";

export type Lang = "en" | "vi";

export const languages: { code: Lang; name: string }[] = [
  { code: "en", name: "English" },
  { code: "vi", name: "Tiếng Việt" },
];

const dicts: Record<Lang, Record<string, string>> = { en: {}, vi };
const patterns: Record<Lang, [RegExp, string][]> = { en: [], vi: viPatterns };

function initial(): Lang {
  try {
    const saved = localStorage.getItem("lang");
    if (saved && saved in dicts) return saved as Lang;
  } catch {}
  return navigator.language?.toLowerCase().startsWith("vi") ? "vi" : "en";
}

let lang: Lang = initial();
document.documentElement.lang = lang;

export function getLang(): Lang {
  return lang;
}

export function setLang(next: Lang) {
  lang = next;
  document.documentElement.lang = next;
  try { localStorage.setItem("lang", next); } catch {}
}

/** Locale for numbers and dates. */
export function locale(): string {
  return lang === "vi" ? "vi-VN" : "en-US";
}

function fill(text: string, vars?: Record<string, string | number>): string {
  if (!vars) return text;
  return text.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m));
}

/** UI text: English in code, translated for the current language. */
export function t(en: string, vars?: Record<string, string | number>): string {
  return fill(dicts[lang][en] ?? en, vars);
}

/** A message that came from the backend (English, may carry values). */
export function tr(msg: string): string {
  const d = dicts[lang];
  if (d[msg]) return d[msg];
  for (const [re, out] of patterns[lang]) if (re.test(msg)) return msg.replace(re, out);
  return msg;
}
