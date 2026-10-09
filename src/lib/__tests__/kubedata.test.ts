import { describe, expect, it } from "vitest";
import { ageOf, isContext, isName, matches, portProblem, shellLine, toneOf, type KubePod, RESOURCE_KINDS, matchesResource, resourceTone } from "../kubedata";

const pod = (over: Partial<KubePod> = {}): KubePod => ({ name: "web-1", namespace: "shop", status: "Running", ready: "1/1", restarts: 0, node: "n1", ip: null, created: null, containers: ["app"], owner: "ReplicaSet/web", ...over });

describe("names", () => {
  it("accepts what Kubernetes allows and nothing a shell could use", () => {
    for (const ok of ["web", "web-7d9f-abc", "a.b-c", "0abc"]) expect(isName(ok), ok).toBe(true);
    for (const bad of ["", "Web", "-a", "a-", "a_b", "a b", "a;b", "$(x)", "a/b", "a'b", "a\n", "x".repeat(254)]) expect(isName(bad), bad).toBe(false);
    for (const ok of ["minikube", "arn:aws:eks:eu-west-1:123456789012:cluster/prod", "gke_proj_europe-west1-b_main", "user@cluster.example.com"]) expect(isContext(ok), ok).toBe(true);
    for (const bad of ["", "-x", "a b", "$(id)", "`id`", "a'b", "--all"]) expect(isContext(bad), bad).toBe(false);
  });
});

describe("a pod's state", () => {
  it("colours it", () => {
    expect(toneOf(pod())).toBe("good");
    expect(toneOf(pod({ ready: "1/2" }))).toBe("warn");
    expect(toneOf(pod({ status: "CrashLoopBackOff", ready: "0/1" }))).toBe("bad");
    expect(toneOf(pod({ status: "ImagePullBackOff" }))).toBe("bad");
    expect(toneOf(pod({ status: "Pending" }))).toBe("warn");
    expect(toneOf(pod({ status: "Terminating" }))).toBe("warn");
    expect(toneOf(pod({ status: "Succeeded", ready: "0/1" }))).toBe("muted");
  });

  it("says its age in one unit", () => {
    const now = Date.parse("2026-10-07T12:00:00Z");
    expect(ageOf("2026-10-07T11:59:30Z", now)).toBe("30s");
    expect(ageOf("2026-10-07T11:30:00Z", now)).toBe("30m");
    expect(ageOf("2026-10-07T07:00:00Z", now)).toBe("5h");
    expect(ageOf("2026-10-01T12:00:00Z", now)).toBe("6d");
    expect(ageOf(null, now)).toBe("—");
    expect(ageOf("nonsense", now)).toBe("—");
    expect(ageOf("2026-10-08T00:00:00Z", now)).toBe("0s");
  });

  it("searches name, namespace, status, node and owner", () => {
    expect(matches(pod(), "")).toBe(true);
    expect(matches(pod(), "WEB")).toBe(true);
    expect(matches(pod(), "shop")).toBe(true);
    expect(matches(pod(), "replicaset")).toBe(true);
    expect(matches(pod(), "n1")).toBe(true);
    expect(matches(pod(), "nothing")).toBe(false);
  });
});

describe("the shell line", () => {
  it("opens bash or sh in the pod, naming the container when asked", () => {
    expect(shellLine("prod", "shop", "web-1")).toBe("kubectl --context 'prod' -n shop exec -it web-1 -- sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'");
    expect(shellLine("prod", "shop", "web-1", "app")).toContain("exec -it web-1 -c app --");
    expect(shellLine("arn:aws:eks:x/y", "shop", "web-1")).toContain("--context 'arn:aws:eks:x/y'");
  });

  it("is null for anything that is not a name, so nothing odd is ever typed", () => {
    expect(shellLine("prod", "shop", "a;b")).toBeNull();
    expect(shellLine("prod", "$(id)", "web")).toBeNull();
    expect(shellLine("--all", "shop", "web")).toBeNull();
    expect(shellLine("prod", "shop", "web", "x y")).toBeNull();
  });

  it("checks ports", () => {
    expect(portProblem("8080")).toBeNull();
    expect(portProblem("1")).toBeNull();
    expect(portProblem("65535")).toBeNull();
    for (const bad of ["", "0", "65536", "-1", "80a", "8 0", "999999"]) expect(portProblem(bad), bad).not.toBeNull();
  });
});

describe("the other kinds", () => {
  it("names the kinds exactly as the backend does (kube::Kind), and never offers Secrets", () => {
    expect(RESOURCE_KINDS.map((k) => k.value)).toEqual(["deployments", "stateful_sets", "daemon_sets", "services", "config_maps", "jobs", "cron_jobs", "ingresses", "events", "nodes"]);
    expect(RESOURCE_KINDS.some((k) => /secret/i.test(k.value + k.label))).toBe(false);
  });

  it("searches name, namespace, status and every column", () => {
    const r = { name: "web", namespace: "shop", status: "2/3 ready", created: null, details: [{ label: "Ports", value: "80/TCP, 443/TCP" }] };
    for (const q of ["", "WEB", "shop", "2/3", "443"]) expect(matchesResource(r, q), q).toBe(true);
    expect(matchesResource(r, "postgres")).toBe(false);
  });

  it("colours the status of each kind", () => {
    const t = (kind: Parameters<typeof resourceTone>[0], status: string) => resourceTone(kind, { status });
    expect([t("deployments", "3/3 ready"), t("deployments", "2/3 ready"), t("deployments", "0/3 ready"), t("deployments", "0/0 ready")]).toEqual(["good", "warn", "bad", "good"]);
    expect([t("nodes", "Ready"), t("nodes", "NotReady"), t("nodes", "Unknown")]).toEqual(["good", "bad", "bad"]);
    expect([t("jobs", "Complete"), t("jobs", "Failed"), t("jobs", "Running")]).toEqual(["good", "bad", "warn"]);
    expect([t("events", "Warning"), t("events", "Normal")]).toEqual(["warn", "muted"]);
    expect([t("cron_jobs", "Suspended"), t("cron_jobs", "Scheduled"), t("services", "ClusterIP")]).toEqual(["muted", "good", "muted"]);
  });
});
