import { describe, expect, it } from "vitest";
import { hostItems, type HostLike } from "../completion/hosts";

const hosts: HostLike[] = [
  { id: "1", label: "web-01", hostname: "web-01.example.com", port: 22, username: "deploy" },
  { id: "2", label: "web-02", hostname: "10.0.0.12", port: 2222, username: "ops" },
  { id: "3", label: "db prod", hostname: "db.example.com", port: 22 },
  { id: "4", label: "router", hostname: "192.168.1.1", port: 23, protocol: "telnet" },
  { id: "5", label: "weird", hostname: "bad host;rm", port: 22 },
];
const at = (line: string) => hostItems(line, hosts);

describe("completing saved hosts after ssh and scp", () => {
  it("offers hosts whose name matches, with what the command needs", () => {
    const r = at("ssh we");
    expect(r.map((i) => i.label)).toEqual(["web-01", "web-02"]);
    expect(r[0].insert).toBe("deploy@web-01.example.com ");
    expect(r[0].erase).toBe(2);
    expect(r[0].group).toBe("Hosts");
  });
  it("puts the port in front when it is not 22 and an option is allowed", () => {
    expect(at("ssh web-02")[0].insert).toBe("-p 2222 ops@10.0.0.12 ");
    expect(at("sftp web-02")[0].insert).toBe("-P 2222 ops@10.0.0.12 ");
    expect(at("ssh -v web-02")[0].insert).toBe("-p 2222 ops@10.0.0.12 ");
  });
  it("scp targets end in a colon, and a host without a user is just the name", () => {
    expect(at("scp file.txt db")[0].insert).toBe("db.example.com:");
    expect(at("scp -r web-01")[0].insert).toBe("deploy@web-01.example.com:");
    // later words of scp may be hosts, but the port can't go in front of a file
    expect(at("scp a.txt web-02")[0].insert).toBe("ops@10.0.0.12:");
  });
  it("matches by host name as well as by label", () => {
    expect(at("ssh 10.0.0")[0].label).toBe("web-02");
    expect(at("ssh deploy@web")[0].label).toBe("web-01");
  });
  it("sees through sudo", () => {
    expect(at("sudo ssh db")).toHaveLength(1);
  });
  it("stays out of the way", () => {
    expect(at("ls we")).toEqual([]);
    expect(at("ssh -i ")).toEqual([]);
    expect(at("ssh -p 22")).toEqual([]);
    expect(at("ssh web-01 uptime")).toEqual([]);
    expect(at("ssh ./key")).toEqual([]);
    expect(at("ssh -")).toEqual([]);
    expect(at("scp host:/tmp/x")).toEqual([]);
    expect(at("ssh 'we")).toEqual([]);
  });
  it("leaves out hosts that are not SSH and names that are not safe to type", () => {
    expect(at("ssh rout")).toEqual([]);
    expect(at("ssh weird")).toEqual([]);
  });
  it("lists at most a few", () => {
    const many = Array.from({ length: 30 }, (_, i): HostLike => ({ id: String(i), label: `node-${i}`, hostname: `node-${i}.x`, port: 22 }));
    expect(hostItems("ssh node", many).length).toBeLessThanOrEqual(6);
  });
});
