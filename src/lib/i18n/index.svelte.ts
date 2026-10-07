// Translating the app's chrome. `t("rail.hosts")` gives the text in the chosen language, English where a language
// has no entry for it. The language is a per-computer setting ("auto" follows the system), and `t` reads it
// reactively, so changing it redraws what is on screen without a restart.
import { settings } from "$lib/stores/settings.svelte";
import { de } from "./de";
import { en, type Key } from "./en";
import { es } from "./es";
import { fa } from "./fa";
import { ja } from "./ja";
import { pt } from "./pt";
import { ru } from "./ru";
import { zh } from "./zh";

export type { Key };

export const LOCALES: { code: string; name: string; messages: Partial<Record<Key, string>> }[] = [
  { code: "en", name: "English", messages: en },
  { code: "es", name: "Español", messages: es },
  { code: "de", name: "Deutsch", messages: de },
  { code: "pt", name: "Português", messages: pt },
  { code: "ru", name: "Русский", messages: ru },
  { code: "zh", name: "中文", messages: zh },
  { code: "ja", name: "日本語", messages: ja },
  { code: "fa", name: "فارسی", messages: fa },
];

/** The language to use: the chosen one, or for "auto" the first of the system's languages that the app has. */
export function pickLocale(pref: string, system: readonly string[]): string {
  const have = new Set(LOCALES.map((l) => l.code));
  if (pref !== "auto" && have.has(pref)) return pref;
  for (const tag of system) {
    const base = tag.toLowerCase().split(/[-_]/)[0];
    if (have.has(base)) return base;
  }
  return "en";
}

/** `{name}` placeholders filled in; one with no value is left as written. */
export function fill(text: string, vars?: Record<string, string | number>): string {
  return vars ? text.replace(/\{(\w+)\}/g, (whole, k: string) => (k in vars ? String(vars[k]) : whole)) : text;
}

/** The text for `key` in `locale`, falling back to English. Not reactive: `t` is. */
export function translate(locale: string, key: Key, vars?: Record<string, string | number>): string {
  const messages = LOCALES.find((l) => l.code === locale)?.messages;
  return fill(messages?.[key] ?? en[key], vars);
}

const system = () => (typeof navigator === "undefined" ? [] : navigator.languages?.length ? navigator.languages : [navigator.language]);

/** The language in use now. */
export const locale = {
  get code(): string {
    return pickLocale(settings.prefs.language, system());
  },
};

export function t(key: Key, vars?: Record<string, string | number>): string {
  return translate(locale.code, key, vars);
}
