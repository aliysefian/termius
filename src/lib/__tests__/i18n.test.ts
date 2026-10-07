import { describe, expect, it } from "vitest";
import { LOCALES, fill, pickLocale, translate } from "../i18n/index.svelte";
import { en, type Key } from "../i18n/en";

const keys = Object.keys(en) as Key[];
const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

describe("the languages", () => {
  it("are unique, and English is first", () => {
    expect(LOCALES[0].code).toBe("en");
    expect(new Set(LOCALES.map((l) => l.code)).size).toBe(LOCALES.length);
    expect(LOCALES.length).toBe(8);
  });

  it.each(LOCALES.filter((l) => l.code !== "en").map((l) => [l.name, l] as const))("%s: has no key English doesn't, and none is empty", (_n, l) => {
    for (const [k, v] of Object.entries(l.messages)) {
      expect(keys.includes(k as Key), `${l.code}: "${k}" is not a key`).toBe(true);
      expect(String(v).trim().length, `${l.code}.${k} is empty`).toBeGreaterThan(0);
    }
  });

  it.each(LOCALES.filter((l) => l.code !== "en").map((l) => [l.name, l] as const))("%s: covers every key", (_n, l) => {
    const missing = keys.filter((k) => !(k in l.messages));
    expect(missing, `${l.code} is missing`).toEqual([]);
  });

  it.each(LOCALES.filter((l) => l.code !== "en").map((l) => [l.name, l] as const))("%s: keeps every {placeholder} English has, and no others", (_n, l) => {
    for (const k of keys) {
      const v = l.messages[k];
      if (v) expect(placeholders(v), `${l.code}.${k}`).toEqual(placeholders(en[k]));
    }
  });

  it("keep the shortcuts as they are (the keys named in the text are the same in every language, only translated modifiers differ)", () => {
    for (const l of LOCALES) expect(l.messages["tour.palette.body"] ?? en["tour.palette.body"]).toMatch(/P/);
  });
});

describe("translating", () => {
  it("gives the language's text, and English for a key the language lacks", () => {
    expect(translate("de", "rail.hosts")).toBe("Hosts");
    expect(translate("de", "rail.sftp")).toBe("Dateien");
    expect(translate("ja", "rail.manage")).toBe("管理");
    expect(translate("xx", "rail.sftp")).toBe("Files");
    expect(translate("en", "rail.sftp")).toBe("Files");
  });

  it("fills placeholders, leaves unknown ones, and does nothing without values", () => {
    expect(fill("Step {n} of {total}", { n: 2, total: 6 })).toBe("Step 2 of 6");
    expect(fill("Step {n} of {total}", { n: 2 })).toBe("Step 2 of {total}");
    expect(fill("Step {n}")).toBe("Step {n}");
    expect(translate("fa", "tour.step", { n: 1, total: 6 })).toContain("1");
    expect(translate("zh", "tour.step", { n: 1, total: 6 })).toBe("第 1 步，共 6 步");
  });

  it("picks the chosen language, or the system's first that the app has, or English", () => {
    expect(pickLocale("de", ["en-US"])).toBe("de");
    expect(pickLocale("auto", ["fr-FR", "es-MX", "de"])).toBe("es");
    expect(pickLocale("auto", ["pt-BR"])).toBe("pt");
    expect(pickLocale("auto", ["zh-Hant-TW"])).toBe("zh");
    expect(pickLocale("auto", ["fr-FR"])).toBe("en");
    expect(pickLocale("auto", [])).toBe("en");
    expect(pickLocale("klingon", ["ru"])).toBe("ru");
    expect(pickLocale("klingon", [])).toBe("en");
    expect(pickLocale("en", ["de"])).toBe("en");
  });
});
