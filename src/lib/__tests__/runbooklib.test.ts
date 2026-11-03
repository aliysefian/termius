import { describe, expect, it } from "vitest";
import { ago, buildLibrary, categoryChoices, copyName, favorites, groupByCategory, recent, searchLibrary, toggleFavorite } from "../runbooklib";
import { KINDS, KIND_BY, dockerFields, fieldsFor, keysFor, summary, valuesFor } from "../runbookschema";
import { BLANK, TEMPLATES, searchTemplates, templateFrom } from "../runbooktemplates";
import { kindOf, load, newStep, STEP_KINDS } from "../runbookdoc";

const rec = (id: string, name: string, body: string, updated = 100) => ({ id, updated_at: updated, data: { name, body } });
const body = (name: string, extra = "") => `name: ${name}\n${extra}steps:\n  - run: ls\n`;

describe("the library", () => {
  const records = [
    rec("1", "Deploy app", body("Deploy app", "description: Ships the app\ncategory: Deployments\n"), 300),
    rec("2", "Clean up", body("Clean up", "description: Removes temp files\ncategory: Maintenance\n"), 200),
    rec("3", "Mine", body("Mine"), 400),
    rec("4", "Zed", body("Zed", "category: Zeta team\n"), 50),
    rec("5", "Broken", "name: [", 10),
    { id: "6", updated_at: 1, deleted: true, data: { name: "Gone", body: body("Gone") } },
  ];
  const items = buildLibrary(records, ["2"]);

  it("reads description and category from the document and leaves out deleted ones", () => {
    expect(items.map((i) => i.id)).toEqual(["1", "2", "3", "4", "5"]);
    expect(items[0]).toMatchObject({ description: "Ships the app", category: "Deployments", steps: 1, favorite: false, broken: false });
    expect(items[1].favorite).toBe(true);
    expect(items[2].category).toBe("Custom");
    expect(items[4]).toMatchObject({ broken: true, steps: 0 });
  });

  it("searches name, description and category, every word", () => {
    expect(searchLibrary(items, "temp").map((i) => i.name)).toEqual(["Clean up"]);
    expect(searchLibrary(items, "ships deploy").map((i) => i.name)).toEqual(["Deploy app"]);
    expect(searchLibrary(items, "zeta").map((i) => i.name)).toEqual(["Zed"]);
    expect(searchLibrary(items, "")).toHaveLength(5);
  });

  it("groups by category: the standard ones in order, then the person's own", () => {
    expect(groupByCategory(items).map((g) => g.name)).toEqual(["Deployments", "Maintenance", "Custom", "Zeta team"]);
    expect(groupByCategory(items).find((g) => g.name === "Custom")!.items.map((i) => i.name)).toEqual(["Broken", "Mine"]);
    expect(categoryChoices(items)).toEqual(["Deployments", "Maintenance", "Monitoring", "Backups", "Security", "Custom", "Zeta team"]);
  });

  it("finds the recent and the favorite ones", () => {
    expect(recent(items, 2).map((i) => i.name)).toEqual(["Mine", "Deploy app"]);
    expect(favorites(items).map((i) => i.name)).toEqual(["Clean up"]);
    expect(toggleFavorite(["2"], "1")).toEqual(["2", "1"]);
    expect(toggleFavorite(["2", "1"], "2")).toEqual(["1"]);
  });

  it("names a copy that isn't taken, and says when something changed", () => {
    expect(copyName("A", [])).toBe("A (copy)");
    expect(copyName("A", ["A (copy)", "A (copy 2)"])).toBe("A (copy 3)");
    const now = 1_700_000_000_000;
    expect(ago(1_700_000_000 - 30, now)).toBe("just now");
    expect(ago(1_700_000_000 - 600, now)).toBe("10 min ago");
    expect(ago(1_700_000_000 - 7200, now)).toBe("2 h ago");
    expect(ago(1_700_000_000 - 100_000, now)).toBe("yesterday");
    expect(ago(1_700_000_000 - 86400 * 5, now)).toBe("5 days ago");
  });
});

describe("the step kinds", () => {
  it("describe every kind the document knows, with a field for what it needs", () => {
    expect(KINDS.map((k) => k.kind)).toEqual([...STEP_KINDS]);
    for (const k of STEP_KINDS) {
      const info = KIND_BY[k];
      expect(info.label && info.about && info.icon).toBeTruthy();
      const step = newStep(k);
      const body = step[k] as Record<string, unknown>;
      // Every key of a new step's settings is a field the editor draws (for a plain run there are none: the command is the step).
      if (k !== "run") for (const key of Object.keys(body)) expect(info.fields.map((f) => f.key), `${k}.${key}`).toContain(key);
    }
    expect(KIND_BY.upload.local && KIND_BY.download.local && KIND_BY.http.local && KIND_BY.tcp.local).toBe(true);
    expect(KIND_BY.run.local || KIND_BY.docker.local).toBe(false);
  });

  it("shows a Docker step only the fields its action uses", () => {
    expect(dockerFields("pull")).toEqual(["image"]);
    expect(fieldsFor("docker", { action: "restart" }).map((f) => f.key)).toEqual(["action", "container", "sudo"]);
    expect(fieldsFor("docker", { action: "compose_up" }).map((f) => f.key)).toEqual(["action", "file", "project", "sudo"]);
    expect(fieldsFor("docker", { action: "exec" }).map((f) => f.key)).toEqual(["action", "container", "command", "sudo"]);
    expect(fieldsFor("tcp", {}).length).toBe(KIND_BY.tcp.fields.length);
  });

  it("says in a line what a step does", () => {
    for (const k of STEP_KINDS) expect(summary(newStep(k), k).length).toBeGreaterThan(3);
    expect(summary({ run: "systemctl restart   nginx" }, "run")).toBe("systemctl restart nginx");
    expect(summary({ http: { url: "https://x/", expect_status: 204 } }, "http")).toBe("GET https://x/ expecting 204");
    expect(summary({ http: { url: "https://x/", method: "head" } }, "http")).toBe("HEAD https://x/ expecting 2xx or 3xx");
    expect(summary({ docker: { action: "compose_up", file: "/a/c.yml" } }, "docker")).toBe("docker compose up /a/c.yml");
    expect(summary({ name: "x" }, null)).toMatch(/doesn't say/);
    expect(summary({ run: "x".repeat(200) }, "run").endsWith("…")).toBe(true);
  });

  it("suggests the keys that are valid where the cursor is", () => {
    expect(keysFor([])).toContain("steps");
    expect(keysFor(["steps"])).toEqual(expect.arrayContaining(["name", "run", "docker", "when", "timeout_secs"]));
    expect(keysFor(["steps", "http"])).toEqual(["url", "method", "expect_status", "contains", "timeout_secs", "every_secs", "wait_secs"]);
    expect(keysFor(["defaults"])).toContain("strategy");
    expect(keysFor(["params"])).toContain("kind");
    expect(keysFor(["steps", "when"])).toContain("exit_not");
    expect(keysFor(["nothing"])).toEqual([]);
    expect(valuesFor(["steps"], "on_error")).toEqual(["stop", "continue"]);
    expect(valuesFor(["params"], "kind")).toContain("dir");
    expect(valuesFor(["steps", "docker"], "action")).toContain("compose_up");
  });
});

describe("the templates", () => {
  it("are all read, with a category, a badge and the variables they ask for", () => {
    expect(TEMPLATES.length).toBeGreaterThanOrEqual(12);
    for (const t of TEMPLATES) {
      expect(load(t.body).model, t.id).not.toBeNull();
      expect(["Deployments", "Maintenance", "Monitoring", "Backups", "Security"]).toContain(t.category);
      expect(["read-only", "changes"]).toContain(t.effect);
      expect(t.description.length).toBeGreaterThan(20);
      expect(t.steps).toBeGreaterThan(0);
      expect(kindOf(load(t.body).model!.steps[0])).not.toBeNull();
    }
    const clean = TEMPLATES.find((t) => t.id === "clean-temp-files")!;
    expect(clean.effect).toBe("changes");
    expect(clean.variables.map((v) => v.name)).toEqual(["folder", "days"]);
    expect(TEMPLATES.find((t) => t.id === "check-disk-usage")!.effect).toBe("read-only");
    const dl = TEMPLATES.find((t) => t.id === "download-files")!;
    expect(dl.variables.find((v) => v.name === "destination")).toMatchObject({ kind: "dir", required: true });
  });

  it("are found by search and category", () => {
    expect(searchTemplates(TEMPLATES, "docker", null).length).toBeGreaterThan(0);
    expect(searchTemplates(TEMPLATES, "", "Backups").every((t) => t.category === "Backups")).toBe(true);
    expect(searchTemplates(TEMPLATES, "zzzz", null)).toEqual([]);
    expect(templateFrom("x", "name: [")).toBeNull();
    expect(load(BLANK).model?.steps).toHaveLength(1);
  });
});

import { yamlContext } from "../runbookschema";

describe("where the cursor is in YAML", () => {
  const doc = `name: x
defaults:
  retries: 1
  
params:
  - name: v
    kind: text
    
steps:
  - name: Check
    http:
      url: https://x/
      
    on_error: stop
  - name: Next
    when:
      
`;
  const at = (needle: string, nth = 0) => {
    let from = -1;
    for (let i = 0; i <= nth; i++) from = doc.indexOf(needle, from + 1);
    return from + needle.length;
  };

  it("finds the mapping for keys at each level", () => {
    expect(yamlContext("", 0).path).toEqual([]);
    expect(yamlContext(doc, at("name: x\n")).path).toEqual([]);
    expect(yamlContext(doc, at("  retries: 1\n  ")).path).toEqual(["defaults"]);
    expect(yamlContext(doc, at("    kind: text\n    ")).path).toEqual(["params"]);
    expect(yamlContext(doc, at("      url: https://x/\n      ")).path).toEqual(["steps", "http"]);
    expect(yamlContext(doc, at("    on_error: stop\n  - ")).path).toEqual(["steps"]);
    expect(yamlContext(doc, doc.length - 1).path).toEqual(["steps", "when"]);
  });

  it("knows when a value is being typed, and what has been typed", () => {
    const c = yamlContext("steps:\n  - name: x\n    on_error: con", "steps:\n  - name: x\n    on_error: con".length);
    expect(c).toEqual({ path: ["steps"], valueOf: "on_error", prefix: "con" });
    const k = yamlContext("steps:\n  - na", "steps:\n  - na".length);
    expect(k).toEqual({ path: ["steps"], valueOf: null, prefix: "na" });
  });
});
