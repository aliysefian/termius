// Hosts from a cloud or tool: what each one's command-line program prints, turned into hosts to add, and a
// refresh that reports what was added, changed and no longer found without overwriting anything the person
// edited. Pure; the window runs the programs (src-tauri/src/inventory.rs) and applies the plan.

import type { Host, HostSource } from "$lib/types";

export type Provider = "aws" | "gcp" | "azure" | "digitalocean" | "hetzner" | "tailscale" | "kubernetes" | "terraform" | "scan";

export interface Found {
  provider: Provider;
  /** The provider's own name for the machine. Stable across refreshes. */
  id: string;
  label: string;
  hostname: string;
  port: number;
  /** "rdp" for Windows machines; absent for SSH. */
  protocol?: "rdp";
  /** Slash-separated group, e.g. "AWS/eu-west-1". */
  group: string;
  tags: string[];
}

export const PROVIDER_NAMES: Record<Provider, string> = {
  aws: "AWS EC2",
  gcp: "Google Cloud",
  azure: "Azure",
  digitalocean: "DigitalOcean",
  hetzner: "Hetzner",
  tailscale: "Tailscale",
  kubernetes: "Kubernetes nodes",
  terraform: "Terraform",
  scan: "Network scan",
};

type Json = Record<string, unknown>;
const obj = (x: unknown): Json => (x && typeof x === "object" && !Array.isArray(x) ? (x as Json) : {});
const arr = (x: unknown): unknown[] => (Array.isArray(x) ? x : []);
const str = (x: unknown): string => (typeof x === "string" ? x.trim() : "");

/** What to put on a line as a host name: letters, digits and . _ - : only (an address or a DNS name). */
const SAFE_HOST = /^[A-Za-z0-9._:-]{1,253}$/;
const MAX_TAGS = 8;
const tagList = (pairs: [string, string][]) => pairs.filter(([k]) => k.toLowerCase() !== "name").slice(0, MAX_TAGS).map(([k, v]) => (v ? `${k}=${v}` : k));

/** The address to use: public unless private is preferred, falling back to the other. */
function pick(publicIp: string, privateIp: string, preferPrivate: boolean): string {
  return (preferPrivate ? privateIp || publicIp : publicIp || privateIp) || "";
}

function make(f: Omit<Found, "port"> & { port?: number }): Found | null {
  if (!f.id || !f.hostname || !SAFE_HOST.test(f.hostname)) return null;
  return { port: f.protocol === "rdp" ? 3389 : 22, ...f, label: f.label || f.id, tags: f.tags.map((t) => t.slice(0, 60)) };
}

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    throw new Error("That isn't JSON. Check the program printed a list and nothing else.");
  }
}

export interface Options {
  preferPrivate?: boolean;
  /** The region, project or context asked for; goes into the group name. */
  scope?: string;
}

// -- the providers -----------------------------------------------------------------

export function parseTailscale(text: string, o: Options = {}): Found[] {
  const peers = obj(obj(parseJson(text)).Peer);
  const out: Found[] = [];
  for (const p of Object.values(peers).map(obj)) {
    const ips = arr(p.TailscaleIPs).map(str);
    const v4 = ips.find((i) => /^\d+\.\d+\.\d+\.\d+$/.test(i)) ?? ips[0] ?? "";
    const dns = str(p.DNSName).replace(/\.$/, "");
    const name = str(p.HostName);
    const tags = [...arr(p.Tags).map((t) => str(t).replace(/^tag:/, "")), ...(str(p.OS) ? [str(p.OS)] : []), ...(p.Online === false ? ["offline"] : [])];
    // The MagicDNS name keeps working when the address changes; the address works without MagicDNS.
    const f = make({ provider: "tailscale", id: name || dns || v4, label: name, hostname: o.preferPrivate ? v4 || dns : dns || v4, group: "Tailscale", tags });
    if (f && name) out.push(f);
  }
  return out;
}

const AWS_GONE = new Set(["terminated", "shutting-down"]);

export function parseAws(text: string, o: Options = {}): Found[] {
  const out: Found[] = [];
  for (const r of arr(obj(parseJson(text)).Reservations)) {
    for (const i of arr(obj(r).Instances).map(obj)) {
      if (AWS_GONE.has(str(obj(i.State).Name))) continue;
      const tags = arr(i.Tags).map(obj);
      const named = tags.find((t) => str(t.Key) === "Name");
      const az = str(obj(i.Placement).AvailabilityZone);
      const region = o.scope || az.replace(/[a-z]$/, "");
      const windows = str(i.Platform).toLowerCase() === "windows";
      const f = make({
        provider: "aws",
        id: str(i.InstanceId),
        label: str(named?.Value),
        hostname: pick(str(i.PublicIpAddress), str(i.PrivateIpAddress), !!o.preferPrivate),
        protocol: windows ? "rdp" : undefined,
        group: `AWS${region ? `/${region}` : ""}`,
        tags: tagList(tags.map((t) => [str(t.Key), str(t.Value)])),
      });
      if (f) out.push(f);
    }
  }
  return out;
}

export function parseGcp(text: string, o: Options = {}): Found[] {
  const out: Found[] = [];
  for (const i of arr(parseJson(text)).map(obj)) {
    const nic = obj(arr(i.networkInterfaces)[0]);
    const nat = str(obj(arr(nic.accessConfigs)[0]).natIP);
    const zone = str(i.zone).split("/").pop() ?? "";
    const region = zone.replace(/-[a-z]$/, "");
    const f = make({
      provider: "gcp",
      id: str(i.name),
      label: str(i.name),
      hostname: pick(nat, str(nic.networkIP), !!o.preferPrivate),
      group: `Google Cloud${o.scope ? `/${o.scope}` : ""}${region ? `/${region}` : ""}`,
      tags: tagList(Object.entries(obj(i.labels)).map(([k, v]) => [k, str(v)])),
    });
    if (f) out.push(f);
  }
  return out;
}

export function parseAzure(text: string, o: Options = {}): Found[] {
  const out: Found[] = [];
  for (const v of arr(parseJson(text)).map(obj)) {
    // `publicIps` and `privateIps` are comma-separated; the first is used.
    const first = (x: unknown) => str(x).split(",")[0].trim();
    const windows = str(obj(obj(obj(v.storageProfile).osDisk)).osType).toLowerCase() === "windows";
    const f = make({
      provider: "azure",
      id: str(v.id) || str(v.name),
      label: str(v.name),
      hostname: pick(first(v.publicIps), first(v.privateIps), !!o.preferPrivate),
      protocol: windows ? "rdp" : undefined,
      group: `Azure${str(v.resourceGroup) ? `/${str(v.resourceGroup)}` : ""}`,
      tags: tagList(Object.entries(obj(v.tags)).map(([k, x]) => [k, str(x)])),
    });
    if (f) out.push(f);
  }
  return out;
}

export function parseDigitalOcean(text: string, o: Options = {}): Found[] {
  const out: Found[] = [];
  for (const d of arr(parseJson(text)).map(obj)) {
    const v4 = arr(obj(d.networks).v4).map(obj);
    const ip = (kind: string) => str(v4.find((n) => str(n.type) === kind)?.ip_address);
    const f = make({
      provider: "digitalocean",
      id: String(d.id ?? ""),
      label: str(d.name),
      hostname: pick(ip("public"), ip("private"), !!o.preferPrivate),
      group: `DigitalOcean${str(obj(d.region).slug) ? `/${str(obj(d.region).slug)}` : ""}`,
      tags: arr(d.tags).map(str).filter(Boolean).slice(0, MAX_TAGS),
    });
    if (f) out.push(f);
  }
  return out;
}

export function parseHetzner(text: string, o: Options = {}): Found[] {
  const out: Found[] = [];
  for (const s of arr(parseJson(text)).map(obj)) {
    const pub = str(obj(obj(s.public_net).ipv4).ip);
    const priv = str(obj(arr(s.private_net)[0]).ip);
    const f = make({
      provider: "hetzner",
      id: String(s.id ?? ""),
      label: str(s.name),
      hostname: pick(pub, priv, !!o.preferPrivate),
      group: `Hetzner${str(obj(obj(s.datacenter).location).name) ? `/${str(obj(obj(s.datacenter).location).name)}` : ""}`,
      tags: tagList(Object.entries(obj(s.labels)).map(([k, v]) => [k, str(v)])),
    });
    if (f) out.push(f);
  }
  return out;
}

export function parseKubernetes(text: string, o: Options = {}): Found[] {
  const out: Found[] = [];
  for (const n of arr(obj(parseJson(text)).items).map(obj)) {
    const meta = obj(n.metadata);
    const addrs = arr(obj(n.status).addresses).map(obj);
    const at = (t: string) => str(addrs.find((a) => str(a.type) === t)?.address);
    const labels = obj(meta.labels);
    const role = Object.keys(labels).find((k) => k.startsWith("node-role.kubernetes.io/"))?.split("/")[1];
    const f = make({
      provider: "kubernetes",
      id: str(meta.name),
      label: str(meta.name),
      hostname: pick(at("ExternalIP"), at("InternalIP"), !!o.preferPrivate),
      group: `Kubernetes${o.scope ? `/${o.scope}` : ""}`,
      tags: role ? [role] : [],
    });
    if (f) out.push(f);
  }
  return out;
}

// -- Terraform: a state file (version 4) or `terraform show -json` ---------------------------

interface Resource {
  type: string;
  name: string;
  values: Json;
}

function terraformResources(doc: Json): Resource[] {
  const out: Resource[] = [];
  // A state file lists each resource with its instances.
  for (const r of arr(doc.resources).map(obj)) {
    if (str(r.mode) && str(r.mode) !== "managed") continue;
    for (const inst of arr(r.instances).map(obj)) out.push({ type: str(r.type), name: str(r.name), values: obj(inst.attributes) });
  }
  // `show -json`: a tree of modules.
  const walk = (m: Json, depth: number) => {
    if (depth > 8) return;
    for (const r of arr(m.resources).map(obj)) if (str(r.mode) === "managed") out.push({ type: str(r.type), name: str(r.name), values: obj(r.values) });
    for (const c of arr(m.child_modules)) walk(obj(c), depth + 1);
  };
  walk(obj(obj(doc.values).root_module), 0);
  return out;
}

const first = (x: unknown) => str(arr(x)[0]);
const tagsOf = (x: unknown) => Object.entries(obj(x)).map(([k, v]): [string, string] => [k, str(v)]);

export function parseTerraform(text: string, o: Options = {}): Found[] {
  const out: Found[] = [];
  for (const r of terraformResources(obj(parseJson(text)))) {
    const v = r.values;
    let f: Found | null = null;
    switch (r.type) {
      case "aws_instance":
        f = make({ provider: "terraform", id: `${r.type}.${r.name}.${str(v.id)}`, label: str(obj(v.tags).Name) || r.name, hostname: pick(str(v.public_ip), str(v.private_ip), !!o.preferPrivate), group: "Terraform/AWS", tags: tagList(tagsOf(v.tags)) });
        break;
      case "google_compute_instance": {
        const nic = obj(arr(v.network_interface)[0]);
        f = make({ provider: "terraform", id: `${r.type}.${r.name}`, label: str(v.name) || r.name, hostname: pick(str(obj(arr(nic.access_config)[0]).nat_ip), str(nic.network_ip), !!o.preferPrivate), group: "Terraform/Google Cloud", tags: [] });
        break;
      }
      case "azurerm_linux_virtual_machine":
      case "azurerm_windows_virtual_machine":
        f = make({ provider: "terraform", id: `${r.type}.${r.name}`, label: str(v.name) || r.name, hostname: pick(str(v.public_ip_address), str(v.private_ip_address), !!o.preferPrivate), protocol: r.type.includes("windows") ? "rdp" : undefined, group: "Terraform/Azure", tags: tagList(tagsOf(v.tags)) });
        break;
      case "digitalocean_droplet":
        f = make({ provider: "terraform", id: `${r.type}.${r.name}`, label: str(v.name) || r.name, hostname: pick(str(v.ipv4_address), str(v.ipv4_address_private), !!o.preferPrivate), group: "Terraform/DigitalOcean", tags: arr(v.tags).map(str).slice(0, MAX_TAGS) });
        break;
      case "hcloud_server":
        f = make({ provider: "terraform", id: `${r.type}.${r.name}`, label: str(v.name) || r.name, hostname: str(v.ipv4_address), group: "Terraform/Hetzner", tags: tagList(tagsOf(v.labels)) });
        break;
      case "linode_instance":
        f = make({ provider: "terraform", id: `${r.type}.${r.name}`, label: str(v.label) || r.name, hostname: str(v.ip_address) || first(v.ipv4), group: "Terraform/Linode", tags: arr(v.tags).map(str).slice(0, MAX_TAGS) });
        break;
      case "vultr_instance":
        f = make({ provider: "terraform", id: `${r.type}.${r.name}`, label: str(v.label) || r.name, hostname: str(v.main_ip), group: "Terraform/Vultr", tags: [] });
        break;
    }
    if (f) out.push(f);
  }
  return out;
}

export function parseScan(hits: { ip: string; banner: string }[]): Found[] {
  return hits.flatMap((h) => {
    const f = make({ provider: "scan", id: h.ip, label: h.ip, hostname: h.ip, group: "Scanned", tags: h.banner ? [h.banner.replace(/^SSH-2\.0-/, "")] : [] });
    return f ? [f] : [];
  });
}

// -- the plan ---------------------------------------------------------------------------

export interface Existing {
  id: string;
  data: Host;
}

export interface Change {
  existing: Existing;
  found: Found;
  /** What changes: only what the person hasn't edited since the import. */
  fields: ("hostname" | "label")[];
}

export interface Plan {
  add: Found[];
  update: Change[];
  /** Imported before, from this provider and scope, and not listed now. Left alone unless the person removes them. */
  gone: Existing[];
  /** In the list already, by another route (same label and address): not added again. */
  duplicate: Found[];
  unchanged: number;
}

export function sourceOf(f: Found, scope: string): HostSource {
  return { provider: f.provider, id: f.id, scope, hostname: f.hostname, label: f.label };
}

/** What to do with `found`, given the hosts there already are. `scope` is the region, project or context listed. */
export function plan(existing: Existing[], found: Found[], provider: Provider, scope = ""): Plan {
  const bySource = new Map<string, Existing>();
  for (const e of existing) {
    const s = e.data.source;
    if (s) bySource.set(`${s.provider}\u0000${s.scope ?? ""}\u0000${s.id}`, e);
  }
  const labelled = new Set(existing.map((e) => `${e.data.label.toLowerCase()}\u0000${e.data.hostname.toLowerCase()}`));
  const result: Plan = { add: [], update: [], gone: [], duplicate: [], unchanged: 0 };
  const seen = new Set<string>();
  for (const f of found) {
    const key = `${f.provider}\u0000${scope}\u0000${f.id}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const have = bySource.get(key);
    if (!have) {
      if (labelled.has(`${f.label.toLowerCase()}\u0000${f.hostname.toLowerCase()}`)) result.duplicate.push(f);
      else result.add.push(f);
      continue;
    }
    const s = have.data.source!;
    const fields: Change["fields"] = [];
    // A field moves only while it is still what the import wrote; if the person changed it, it stays theirs.
    if (f.hostname !== s.hostname && have.data.hostname === s.hostname) fields.push("hostname");
    if (f.label !== s.label && have.data.label === s.label) fields.push("label");
    if (fields.length) result.update.push({ existing: have, found: f, fields });
    else result.unchanged++;
  }
  for (const [key, e] of bySource) {
    const s = e.data.source!;
    if (s.provider === provider && (s.scope ?? "") === scope && !seen.has(key)) result.gone.push(e);
  }
  return result;
}

/** The host record for something found. `identityId` is the credential to sign in with, when one was chosen. */
export function toHost(f: Found, scope: string, identityId?: string): Host {
  const host: Host = { label: f.label, hostname: f.hostname, port: f.port, group: f.group, tags: f.tags, notes: "", source: sourceOf(f, scope) };
  if (f.protocol) host.protocol = f.protocol;
  if (identityId) host.identity_id = identityId;
  return host;
}

/** The host after a refresh: the fields that were not edited take the new values, and the record of the import follows. */
export function applyChange(c: Change): Host {
  // A plain copy (JSON), so this also works on a record held in reactive state.
  const h: Host = JSON.parse(JSON.stringify(c.existing.data));
  const s = { ...h.source! };
  for (const field of c.fields) {
    h[field] = c.found[field];
    s[field] = c.found[field];
  }
  h.source = s;
  return h;
}

// -- exporting for Ansible ---------------------------------------------------------------

const ANSIBLE_NAME = /[^A-Za-z0-9_]/g;

/**
 * An Ansible inventory in its YAML format, written as JSON (which that format also reads): the groups follow the
 * folders of the host list, and each host has its address, port and user name.
 */
export function ansibleInventory(hosts: { label: string; hostname: string; port: number; group: string; username?: string; protocol?: string }[]): string {
  type Node = { hosts: Record<string, Record<string, unknown>>; children: Record<string, Node> };
  const fresh = (): Node => ({ hosts: {}, children: {} });
  const root = fresh();
  const used = new Set<string>();
  for (const h of hosts) {
    if (h.protocol && h.protocol !== "ssh") continue;
    // A host name: its label, made safe, and unique.
    let name = h.label.trim().replace(/[^A-Za-z0-9_.-]/g, "_") || h.hostname;
    while (used.has(name)) name += "_";
    used.add(name);
    let node = root;
    for (const part of h.group.split("/").map((p) => p.trim().replace(ANSIBLE_NAME, "_")).filter(Boolean)) node = node.children[part] ??= fresh();
    const vars: Record<string, unknown> = { ansible_host: h.hostname };
    if (h.port !== 22) vars.ansible_port = h.port;
    if (h.username) vars.ansible_user = h.username;
    node.hosts[name] = vars;
  }
  const clean = (n: Node): Record<string, unknown> => {
    const out: Record<string, unknown> = {};
    if (Object.keys(n.hosts).length) out.hosts = n.hosts;
    if (Object.keys(n.children).length) out.children = Object.fromEntries(Object.entries(n.children).map(([k, v]) => [k, clean(v)]));
    return out;
  };
  return JSON.stringify({ all: clean(root) }, null, 2) + "\n";
}
