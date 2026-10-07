import { describe, expect, it } from "vitest";
import { ansibleInventory, applyChange, parseAws, parseAzure, parseDigitalOcean, parseGcp, parseHetzner, parseKubernetes, parseScan, parseTailscale, parseTerraform, plan, toHost, type Found } from "../inventory";
import type { Host } from "../types";

// Shapes as the programs print them (trimmed to what is read).
const TAILSCALE = JSON.stringify({
  Self: { HostName: "laptop", TailscaleIPs: ["100.64.0.1"], DNSName: "laptop.tail1234.ts.net." },
  Peer: {
    "nodekey:aaa": { HostName: "web-1", DNSName: "web-1.tail1234.ts.net.", TailscaleIPs: ["100.64.0.2", "fd7a::2"], OS: "linux", Online: true, Tags: ["tag:server", "tag:prod"] },
    "nodekey:bbb": { HostName: "nas", DNSName: "nas.tail1234.ts.net.", TailscaleIPs: ["100.64.0.3"], OS: "linux", Online: false },
    "nodekey:ccc": { HostName: "", DNSName: "", TailscaleIPs: [] },
  },
});

const AWS = JSON.stringify({
  Reservations: [
    {
      Instances: [
        { InstanceId: "i-0abc", State: { Name: "running" }, PublicIpAddress: "54.1.2.3", PrivateIpAddress: "10.0.0.5", Placement: { AvailabilityZone: "eu-west-1b" }, Tags: [{ Key: "Name", Value: "api" }, { Key: "env", Value: "prod" }] },
        { InstanceId: "i-0def", State: { Name: "running" }, PrivateIpAddress: "10.0.0.6", Placement: { AvailabilityZone: "eu-west-1a" }, Platform: "windows" },
        { InstanceId: "i-0old", State: { Name: "terminated" }, PrivateIpAddress: "10.0.0.7" },
        { InstanceId: "i-0noip", State: { Name: "stopped" } },
      ],
    },
  ],
});

describe("reading what each program prints", () => {
  it("Tailscale: peers by MagicDNS name, tags and OS, not itself", () => {
    const r = parseTailscale(TAILSCALE);
    expect(r.map((f) => f.label)).toEqual(["web-1", "nas"]);
    expect(r[0]).toMatchObject({ hostname: "web-1.tail1234.ts.net", group: "Tailscale", tags: ["server", "prod", "linux"] });
    expect(r[1].tags).toContain("offline");
    expect(parseTailscale(TAILSCALE, { preferPrivate: true })[0].hostname).toBe("100.64.0.2");
  });

  it("AWS: the public address, or the private one; Windows is RDP; ended instances are skipped", () => {
    const r = parseAws(AWS);
    expect(r.map((f) => f.id)).toEqual(["i-0abc", "i-0def"]);
    expect(r[0]).toMatchObject({ label: "api", hostname: "54.1.2.3", group: "AWS/eu-west-1", port: 22, tags: ["env=prod"] });
    expect(r[1]).toMatchObject({ label: "i-0def", hostname: "10.0.0.6", protocol: "rdp", port: 3389 });
    expect(parseAws(AWS, { preferPrivate: true })[0].hostname).toBe("10.0.0.5");
  });

  it("Google Cloud, Azure, DigitalOcean, Hetzner and Kubernetes", () => {
    const gcp = parseGcp(JSON.stringify([{ name: "vm-1", zone: "https://x/zones/us-central1-a", networkInterfaces: [{ networkIP: "10.1.0.2", accessConfigs: [{ natIP: "35.1.1.1" }] }], labels: { team: "a" } }]), { scope: "proj" });
    expect(gcp[0]).toMatchObject({ id: "vm-1", hostname: "35.1.1.1", group: "Google Cloud/proj/us-central1", tags: ["team=a"] });
    const az = parseAzure(JSON.stringify([{ id: "/subs/x/vm1", name: "vm1", resourceGroup: "rg1", publicIps: "20.1.1.1,20.1.1.2", privateIps: "10.2.0.4", storageProfile: { osDisk: { osType: "Windows" } } }]));
    expect(az[0]).toMatchObject({ label: "vm1", hostname: "20.1.1.1", group: "Azure/rg1", protocol: "rdp" });
    const doc = parseDigitalOcean(JSON.stringify([{ id: 42, name: "drop", networks: { v4: [{ ip_address: "10.9.0.2", type: "private" }, { ip_address: "165.1.1.1", type: "public" }] }, region: { slug: "nyc3" }, tags: ["web"] }]));
    expect(doc[0]).toMatchObject({ id: "42", hostname: "165.1.1.1", group: "DigitalOcean/nyc3", tags: ["web"] });
    const hz = parseHetzner(JSON.stringify([{ id: 7, name: "h1", public_net: { ipv4: { ip: "95.1.1.1" } }, private_net: [{ ip: "10.3.0.2" }], datacenter: { location: { name: "fsn1" } }, labels: { role: "db" } }]));
    expect(hz[0]).toMatchObject({ id: "7", hostname: "95.1.1.1", group: "Hetzner/fsn1", tags: ["role=db"] });
    const k = parseKubernetes(JSON.stringify({ items: [{ metadata: { name: "node-1", labels: { "node-role.kubernetes.io/control-plane": "" } }, status: { addresses: [{ type: "InternalIP", address: "10.4.0.2" }, { type: "Hostname", address: "node-1" }] } }] }), { scope: "prod" });
    expect(k[0]).toMatchObject({ label: "node-1", hostname: "10.4.0.2", group: "Kubernetes/prod", tags: ["control-plane"] });
  });

  it("Terraform: a state file and `terraform show -json`, only the kinds of machine it knows", () => {
    const state = JSON.stringify({
      version: 4,
      resources: [
        { mode: "managed", type: "aws_instance", name: "web", instances: [{ attributes: { id: "i-1", public_ip: "3.3.3.3", private_ip: "10.0.0.1", tags: { Name: "web-tf" } } }] },
        { mode: "managed", type: "digitalocean_droplet", name: "d", instances: [{ attributes: { name: "drop-tf", ipv4_address: "1.2.3.4" } }] },
        { mode: "managed", type: "aws_s3_bucket", name: "b", instances: [{ attributes: { id: "b" } }] },
        { mode: "data", type: "aws_instance", name: "existing", instances: [{ attributes: { public_ip: "9.9.9.9" } }] },
      ],
    });
    expect(parseTerraform(state).map((f) => [f.label, f.hostname, f.group])).toEqual([["web-tf", "3.3.3.3", "Terraform/AWS"], ["drop-tf", "1.2.3.4", "Terraform/DigitalOcean"]]);
    const show = JSON.stringify({ values: { root_module: { resources: [{ mode: "managed", type: "hcloud_server", name: "h", values: { name: "h-tf", ipv4_address: "5.5.5.5" } }], child_modules: [{ resources: [{ mode: "managed", type: "vultr_instance", name: "v", values: { label: "v-tf", main_ip: "6.6.6.6" } }] }] } } });
    expect(parseTerraform(show).map((f) => f.label)).toEqual(["h-tf", "v-tf"]);
  });

  it("a scan's hits", () => {
    expect(parseScan([{ ip: "192.168.1.5", banner: "SSH-2.0-OpenSSH_9.6" }])[0]).toMatchObject({ provider: "scan", id: "192.168.1.5", group: "Scanned", tags: ["OpenSSH_9.6"] });
  });

  it("says plainly when it isn't JSON, and skips what can't be reached or isn't a safe name", () => {
    expect(() => parseAws("not json")).toThrow(/isn't JSON/);
    expect(parseAws("{}")).toEqual([]);
    expect(parseAws(JSON.stringify({ Reservations: [{ Instances: [{ InstanceId: "i-x", State: { Name: "running" }, PublicIpAddress: "1.2.3.4; rm -rf /" }] }] }))).toEqual([]);
    expect(parseGcp("[]")).toEqual([]);
  });
});

const found = (o: Partial<Found> = {}): Found => ({ provider: "aws", id: "i-1", label: "api", hostname: "1.1.1.1", port: 22, group: "AWS/eu", tags: [], ...o });
const rec = (id: string, data: Partial<Host>) => ({ id, data: { label: "x", hostname: "h", port: 22, group: "", tags: [], notes: "", ...data } as Host });

describe("refreshing without overwriting", () => {
  const imported = (f: Found, scope = "eu") => rec("r-" + f.id, { ...toHost(f, scope) });

  it("a first import adds everything and marks where it came from", () => {
    const p = plan([], [found(), found({ id: "i-2", label: "db", hostname: "2.2.2.2" })], "aws", "eu");
    expect(p.add).toHaveLength(2);
    expect(toHost(found(), "eu", "ident").source).toEqual({ provider: "aws", id: "i-1", scope: "eu", hostname: "1.1.1.1", label: "api" });
    expect(toHost(found(), "eu", "ident").identity_id).toBe("ident");
  });

  it("a second run with nothing new changes nothing", () => {
    const f = found();
    expect(plan([imported(f)], [f], "aws", "eu")).toMatchObject({ add: [], update: [], gone: [], unchanged: 1 });
  });

  it("a changed address is updated, a changed name too", () => {
    const f = found();
    const p = plan([imported(f)], [found({ hostname: "9.9.9.9", label: "api-2" })], "aws", "eu");
    expect(p.update).toHaveLength(1);
    expect(p.update[0].fields).toEqual(["hostname", "label"]);
    const after = applyChange(p.update[0]);
    expect([after.hostname, after.label]).toEqual(["9.9.9.9", "api-2"]);
    expect(after.source).toMatchObject({ hostname: "9.9.9.9", label: "api-2" });
  });

  it("what the person edited is never overwritten", () => {
    const f = found();
    const mine = rec("r-i-1", { ...toHost(f, "eu"), label: "my-favourite-server", port: 2222, group: "Mine", notes: "keep" });
    const p = plan([mine], [found({ hostname: "9.9.9.9", label: "api-2" })], "aws", "eu");
    expect(p.update[0].fields).toEqual(["hostname"]);
    const after = applyChange(p.update[0]);
    expect(after).toMatchObject({ label: "my-favourite-server", hostname: "9.9.9.9", port: 2222, group: "Mine", notes: "keep" });
    // Edited address too: nothing to do.
    const both = rec("r-i-1", { ...toHost(f, "eu"), hostname: "my.example.com" });
    expect(plan([both], [found({ hostname: "9.9.9.9" })], "aws", "eu").update).toEqual([]);
  });

  it("what is no longer listed is reported, only for the same provider and scope", () => {
    const a = imported(found());
    const other = imported(found({ id: "i-9", provider: "gcp" }), "eu");
    const elsewhere = imported(found({ id: "i-8" }), "us");
    const p = plan([a, other, elsewhere], [], "aws", "eu");
    expect(p.gone.map((g) => g.id)).toEqual(["r-i-1"]);
  });

  it("a host already in the list by hand is not added twice", () => {
    const mine = rec("m", { label: "api", hostname: "1.1.1.1" });
    const p = plan([mine], [found()], "aws", "eu");
    expect(p.add).toEqual([]);
    expect(p.duplicate).toHaveLength(1);
  });

  it("the same machine twice in one answer is one", () => {
    expect(plan([], [found(), found()], "aws", "eu").add).toHaveLength(1);
  });
});

describe("exporting for Ansible", () => {
  it("groups follow the folders; each host has its address, port and user", () => {
    const out = JSON.parse(
      ansibleInventory([
        { label: "web 1", hostname: "10.0.0.1", port: 22, group: "Production/Web", username: "deploy" },
        { label: "db", hostname: "10.0.0.2", port: 2222, group: "Production" },
        { label: "loose", hostname: "h.example", port: 22, group: "" },
        { label: "desk", hostname: "d", port: 3389, group: "x", protocol: "rdp" },
        { label: "web 1", hostname: "10.0.0.3", port: 22, group: "Production/Web" },
      ]),
    );
    expect(out.all.hosts.loose).toEqual({ ansible_host: "h.example" });
    expect(out.all.children.Production.hosts.db).toEqual({ ansible_host: "10.0.0.2", ansible_port: 2222 });
    expect(out.all.children.Production.children.Web.hosts["web_1"]).toEqual({ ansible_host: "10.0.0.1", ansible_user: "deploy" });
    expect(Object.keys(out.all.children.Production.children.Web.hosts)).toEqual(["web_1", "web_1_"]);
    expect(JSON.stringify(out)).not.toContain("desk");
  });

  it("makes safe group names", () => {
    const out = JSON.parse(ansibleInventory([{ label: "a", hostname: "h", port: 22, group: "My Group/Sub-Group" }]));
    expect(Object.keys(out.all.children)).toEqual(["My_Group"]);
    expect(Object.keys(out.all.children.My_Group.children)).toEqual(["Sub_Group"]);
  });
});
