import { describe, expect, it } from "vitest";
import { addParam, addStep, changeKind, convert, duplicateStep, format, formatOf, kindOf, load, moveParam, moveStep, newStep, positions, removeParam, removeStep, renameInTemplate, renameParam, setDefault, setStepField, setStepId, setTop, updateStep, usedNames, wouldLoseComments, type Edit } from "../runbookdoc";

const yaml = `# Deploy the app
name: Deploy
description: Ships it.
category: Deployments
params:
  - name: service   # which one
    default: nginx
  - name: mode
    choices: [fast, safe]
    default: safe
steps:
  # first, look
  - name: Is it up
    id: up
    run: "systemctl is-active {{service|q}}"
    on_error: continue
  - name: Restart
    run: "sudo systemctl restart {{service|q}}"
    when:
      step: up
      exit_not: 0
`;

const json = JSON.stringify({ name: "J", params: [{ name: "svc", default: "a" }], steps: [{ name: "One", id: "one", run: "echo {{svc}}" }, { name: "Two", run: "echo two", when: { param: "svc", equals: "a" } }] }, null, 2);

const ok = (e: Edit): string => {
  if ("error" in e) throw new Error(e.error);
  return e.text;
};

describe("load", () => {
  it("reads YAML and JSON into the same kind of model", () => {
    const y = load(yaml);
    expect(y.format).toBe("yaml");
    expect(y.model?.name).toBe("Deploy");
    expect(y.model?.category).toBe("Deployments");
    expect(y.model?.steps.map((s) => s.name)).toEqual(["Is it up", "Restart"]);
    expect(y.model?.params).toHaveLength(2);
    expect(y.unsupported).toEqual([]);
    const j = load(json);
    expect(j.format).toBe("json");
    expect(j.model?.steps).toHaveLength(2);
  });

  it("says where a document can't be read", () => {
    const l = load("name: x\nsteps:\n  - run: [unclosed\n");
    expect(l.model).toBeNull();
    expect(l.error?.line).toBeGreaterThanOrEqual(3);
    expect(load("{ nope").error).not.toBeNull();
    expect(load("").error?.message).toMatch(/empty/);
    expect(load("- a\n- b").error?.message).toMatch(/mapping/);
  });

  it("lists keys the builder doesn't know, and says when it can't edit safely", () => {
    const l = load("name: x\nfrob: 1\nsteps:\n  - run: ls\n    wobble: 2\n");
    expect(l.model?.unknownKeys.sort()).toEqual(["frob", "wobble"]);
    const anchored = load("name: x\nsteps:\n  - &a\n    run: ls\n  - *a\n");
    expect(anchored.unsupported[0]).toMatch(/anchors/);
    expect(load("name: x\nsteps:\n  - run: ls\n  - just text\n").unsupported[0]).toMatch(/not a mapping/);
  });

  it("detects the format from the first character", () => {
    expect(formatOf(' {"a":1}')).toBe("json");
    expect(formatOf("name: x")).toBe("yaml");
  });
});

describe("editing keeps what it doesn't touch", () => {
  it("renames the runbook without losing comments or order in YAML", () => {
    const t = ok(setTop(yaml, "name", "Deploy v2"));
    expect(t).toContain("# Deploy the app");
    expect(t).toContain("# first, look");
    expect(t).toContain("# which one");
    expect(t).toContain("name: Deploy v2");
    expect(load(t).model?.steps).toHaveLength(2);
  });

  it("adds, moves, duplicates and removes steps by their place", () => {
    let t = ok(addStep(yaml, newStep("delay"), { index: 1 }));
    expect(load(t).model?.steps.map((s) => s.name)).toEqual(["Is it up", "Pause", "Restart"]);
    t = ok(moveStep(t, 1, 2));
    expect(load(t).model?.steps.map((s) => s.name)).toEqual(["Is it up", "Restart", "Pause"]);
    t = ok(duplicateStep(t, 0));
    const names = load(t).model!.steps.map((s) => s.name);
    expect(names).toEqual(["Is it up", "Is it up (copy)", "Restart", "Pause"]);
    // The copy has an id of its own.
    const ids = load(t).model!.steps.map((s) => s.id).filter(Boolean);
    expect(new Set(ids).size).toBe(ids.length);
    t = ok(removeStep(t, 1));
    expect(load(t).model!.steps).toHaveLength(3);
    expect(t).toContain("# first, look");
    expect("error" in moveStep(t, 0, 9)).toBe(true);
    expect("error" in removeStep(t, 9)).toBe(true);
  });

  it("does the same in JSON, written back in order", () => {
    let t = ok(addStep(json, newStep("tcp")));
    expect(load(t).model!.steps.map((s) => s.name)).toEqual(["One", "Two", "Check a port"]);
    t = ok(moveStep(t, 2, 0));
    expect(load(t).model!.steps[0].name).toBe("Check a port");
    t = ok(setStepField(t, 0, "disabled", true));
    expect(load(t).model!.steps[0].disabled).toBe(true);
    t = ok(setStepField(t, 0, "disabled", false));
    expect(load(t).model!.steps[0].disabled).toBeUndefined();
    expect(formatOf(t)).toBe("json");
  });

  it("changes a field of a step, and removes it when emptied", () => {
    let t = ok(setStepField(yaml, 0, "timeout_secs", 30));
    expect(load(t).model!.steps[0].timeout_secs).toBe(30);
    t = ok(setStepField(t, 0, "timeout_secs", undefined));
    expect(load(t).model!.steps[0].timeout_secs).toBeUndefined();
    t = ok(setStepField(t, 0, "on_error", "stop"));
    expect(load(t).model!.steps[0].on_error).toBe("stop");
  });

  it("replaces a step as a whole and refuses a step that isn't there", () => {
    const t = ok(updateStep(yaml, 1, { name: "Restart it", run: "reboot" }));
    expect(load(t).model!.steps[1]).toEqual({ name: "Restart it", run: "reboot" });
    expect("error" in updateStep(yaml, 5, {})).toBe(true);
  });

  it("gives a step an id, and refuses one that is used", () => {
    expect("error" in setStepId(yaml, 1, "up")).toBe(true);
    const t = ok(setStepId(yaml, 1, "again"));
    expect(load(t).model!.steps[1].id).toBe("again");
    expect(load(ok(setStepId(t, 1, undefined))).model!.steps[1].id).toBeUndefined();
  });

  it("changes a step's kind and keeps its conditions", () => {
    const t = ok(changeKind(yaml, 1, "docker"));
    const s = load(t).model!.steps[1];
    expect(s.run).toBeUndefined();
    expect(kindOf(s)).toBe("docker");
    expect(s.name).toBe("Restart");
    expect(s.when).toEqual({ step: "up", exit_not: 0 });
    expect(Object.keys(s).indexOf("docker")).toBeLessThan(Object.keys(s).indexOf("when"));
  });

  it("edits the rollback list on its own", () => {
    const t = ok(addStep(yaml, { name: "Undo", run: "echo undo" }, { rollback: true }));
    const m = load(t).model!;
    expect(m.rollback.map((s) => s.name)).toEqual(["Undo"]);
    expect(m.steps).toHaveLength(2);
    expect(load(ok(removeStep(t, 0, true))).model!.rollback).toEqual([]);
  });

  it("refuses to edit what it can't read or can't edit safely, and says why", () => {
    expect("error" in setTop("name: x\nsteps: [", "name", "y")).toBe(true);
    const e = setTop("name: x\nsteps:\n  - &a\n    run: ls\n  - *a\n", "name", "y");
    expect("error" in e && e.error).toMatch(/YAML view/);
  });
});

describe("defaults", () => {
  it("writes a default, and drops the block when the last one goes", () => {
    let t = ok(setDefault(yaml, "timeout_secs", 45));
    expect(load(t).model!.defaults).toEqual({ timeout_secs: 45 });
    t = ok(setDefault(t, "retries", 2));
    expect(load(t).model!.defaults).toEqual({ timeout_secs: 45, retries: 2 });
    t = ok(setDefault(t, "timeout_secs", undefined));
    expect(load(t).model!.defaults).toEqual({ retries: 2 });
    t = ok(setDefault(t, "retries", undefined));
    expect(t).not.toContain("defaults");
  });
  it("keeps a list of hosts and a strategy", () => {
    const t = ok(setDefault(ok(setDefault(yaml, "hosts", ["a", "b"])), "strategy", { rolling: { batch_size: 2 } }));
    expect(load(t).model!.defaults).toEqual({ hosts: ["a", "b"], strategy: { rolling: { batch_size: 2 } } });
  });
});

describe("variables", () => {
  it("adds, moves and removes variables, and refuses a name that is taken", () => {
    let t = ok(addParam(yaml, { name: "limit", default: "85" }));
    expect(load(t).model!.params.map((p) => p.name)).toEqual(["service", "mode", "limit"]);
    expect("error" in addParam(t, { name: "mode" })).toBe(true);
    t = ok(moveParam(t, 2, 0));
    expect(load(t).model!.params[0].name).toBe("limit");
    t = ok(removeParam(t, 0));
    t = ok(removeParam(t, 0));
    t = ok(removeParam(t, 0));
    expect(t).not.toContain("params");
    expect("error" in removeParam(t, 0)).toBe(true);
  });

  it("renames a variable and every use of it, and nothing that only looks similar", () => {
    const base = `name: r
params:
  - name: svc
  - name: svc2
steps:
  - run: "restart {{svc|q}} and {{ svc }} but not {{svc2}} or svc"
    id: a
  - run: "x"
    when:
      param: svc
      equals: nginx
  - docker:
      action: restart
      container: "{{svc}}"
`;
    const t = ok(renameParam(base, 0, "service"));
    const m = load(t).model!;
    expect(m.params.map((p) => p.name)).toEqual(["service", "svc2"]);
    expect(m.steps[0].run).toBe("restart {{service|q}} and {{ service }} but not {{svc2}} or svc");
    expect((m.steps[1].when as { param: string }).param).toBe("service");
    expect((m.steps[2].docker as { container: string }).container).toBe("{{service}}");
    expect("error" in renameParam(base, 0, "svc2")).toBe(true);
  });

  it("finds and renames placeholders in a template", () => {
    expect(renameInTemplate("{{a}} {{ a|q }} {{ab}} {a}", "a", "b")).toBe("{{b}} {{ b|q }} {{ab}} {a}");
    expect(renameInTemplate("{{a", "a", "b")).toBe("{{a");
    expect(usedNames("{{ x|q }} {{host}} {{x}} {{y}}")).toEqual(["x", "host", "y"]);
  });
});

describe("conversion", () => {
  it("turns JSON into YAML and back without changing what it says", () => {
    const y = ok(convert(json, "yaml"));
    expect(formatOf(y)).toBe("yaml");
    expect(load(y).model).toEqual(load(json).model);
    const back = ok(convert(y, "json"));
    expect(JSON.parse(back)).toEqual(JSON.parse(json));
  });
  it("keeps comments going YAML → YAML and says when going to JSON would lose them", () => {
    expect(ok(convert(yaml, "yaml"))).toBe(yaml);
    expect(wouldLoseComments(yaml, "json")).toBe(true);
    expect(wouldLoseComments("name: x\nsteps:\n  - run: ls\n", "json")).toBe(false);
    expect(wouldLoseComments(json, "yaml")).toBe(false);
  });
  it("formats without changing the meaning, and says when it can't", () => {
    const messy = 'name:   x\nsteps:\n    -   run:    "ls"\n';
    expect(load(ok(format(messy))).model).toEqual(load(messy).model);
    expect(ok(format('{"name":"x","steps":[{"run":"ls"}]}')).startsWith('{\n  "name"')).toBe(true);
    expect("error" in format("name: [")).toBe(true);
  });
});

describe("positions", () => {
  it("says on which line each step and variable starts", () => {
    const p = positions(yaml);
    expect(p.steps.map((s) => s.line)).toEqual([13, 17]);
    expect(p.params.map((s) => s.line)).toEqual([6, 8]);
    const j = positions(json);
    expect(j.steps).toHaveLength(2);
    expect(j.steps[1].line).toBeGreaterThan(j.steps[0].line);
    expect(positions("name: [")).toEqual({ steps: [], rollback: [], params: [] });
  });
});

describe("new steps", () => {
  it("every kind is a step of exactly that kind", () => {
    for (const k of ["run", "wait", "upload", "download", "delay", "http", "tcp", "docker"] as const) {
      expect(kindOf(newStep(k))).toBe(k);
    }
    expect(kindOf({ name: "x" })).toBeNull();
    expect(kindOf({ run: "a", delay: { secs: 1 } })).toBeNull();
  });
});
