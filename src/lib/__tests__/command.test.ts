import { describe, expect, it } from "vitest";
import { completeLine, parseLine, walk, type Node, type Specs } from "../completion/command";

// The committed specs, as the app will load them.
const files = import.meta.glob<Node>("../completion/specs/*.json", { eager: true, import: "default" });
const byName = new Map<string, Node>();
let index: Record<string, string> = {};
for (const [path, spec] of Object.entries(files)) {
  const name = path.replace(/^.*\//, "").replace(/\.json$/, "");
  if (name === "index") index = spec as unknown as Record<string, string>;
  else if (name !== "SOURCE") byName.set(name, spec);
}
const specs: Specs = {
  known: (c) => c in index,
  get: (c) => byName.get(c),
  names: () => Object.entries(index),
};

const names = (line: string) => completeLine(line, specs).items.map((i) => i.name);
const inserts = (line: string) => completeLine(line, specs).items.map((i) => i.insert);

describe("reading a line", () => {
  it("splits words and keeps quoted text whole", () => {
    expect(parseLine("git commit -m 'a b' ")).toMatchObject({ words: ["git", "commit", "-m", "a b"], current: "", quoted: false });
    expect(parseLine('echo "x y" z')).toMatchObject({ words: ["echo", "x y"], current: "z" });
    expect(parseLine("ls my\\ dir/f")).toMatchObject({ words: ["ls"], current: "my dir/f", raw: "my\\ dir/f" });
  });

  it("knows when it is inside a quote", () => {
    expect(parseLine("git commit -m 'fix the").quoted).toBe(true);
    expect(parseLine('echo "a').quoted).toBe(true);
    expect(parseLine("echo 'a' b").quoted).toBe(false);
    expect(parseLine("echo it\\'s").quoted).toBe(false);
  });

  it("starts again after a pipe, && , || or ;", () => {
    expect(parseLine("cat f | grep -i ")).toMatchObject({ words: ["grep", "-i"] });
    expect(parseLine("make && docker ps")).toMatchObject({ words: ["docker"], current: "ps" });
    expect(parseLine("a || b; c d e")).toMatchObject({ words: ["c", "d"], current: "e" });
    expect(parseLine("echo $(git st")).toMatchObject({ words: ["git"], current: "st" });
  });

  it("keeps quoted operators as text", () => {
    expect(parseLine("grep 'a|b' f")).toMatchObject({ words: ["grep", "a|b"], current: "f" });
    expect(parseLine('echo "a && b" c')).toMatchObject({ words: ["echo", "a && b"], current: "c" });
  });

  it("skips leading VAR=value words and redirections", () => {
    expect(parseLine("FOO=1 BAR=2 git st")).toMatchObject({ words: ["git"], current: "st" });
    expect(parseLine("ls > out.txt -l")).toMatchObject({ words: ["ls"], current: "-l" });
    expect(parseLine("ls 2>&1 -l")).toMatchObject({ words: ["ls"], current: "-l" });
    expect(parseLine("ls > ").redirect).toBe(true);
    expect(parseLine("ls > ou").redirect).toBe(true);
    expect(parseLine("ls >out -").redirect).toBe(false);
  });
});

describe("git", () => {
  it("offers subcommands, narrowed by what is typed", () => {
    expect(names("git ch")).toContain("checkout");
    expect(names("git ch")).not.toContain("commit");
    expect(names("git ")).toContain("commit");
    expect(inserts("git chec")).toContain("checkout ");
  });

  it("describes them", () => {
    const item = completeLine("git chec", specs).items.find((i) => i.name === "checkout");
    expect(item?.description).toMatch(/branch/i);
    expect(item?.kind).toBe("subcommand");
    expect(item?.replaces).toBe(4);
  });

  it("offers the options of the subcommand it is in, not of git", () => {
    expect(names("git commit --am")).toEqual(["--amend"]);
    expect(names("git commit -")).toContain("-m");
    expect(names("git status --am")).toEqual([]);
  });

  it("does not offer an option twice, but does offer a repeatable one", () => {
    expect(names("git commit --amend --am")).toEqual([]);
    expect(names("git commit -m 'x' -")).not.toContain("-m");
  });

  it("is quiet inside a quote and after a redirection", () => {
    expect(names("git commit -m 'fix --am")).toEqual([]);
    expect(names("git log > ch")).toEqual([]);
  });
});

describe("docker", () => {
  it("lists subcommands, and nested ones", () => {
    expect(names("docker ru")).toEqual(expect.arrayContaining(["run"]));
    expect(names("docker compose do")).toEqual(["down"]);
    expect(names("docker compose u")).toEqual(expect.arrayContaining(["up"]));
    expect(names("docker compose up --d")).toEqual(expect.arrayContaining(["--detach"]));
  });

  it("offers a run option and takes the value after one that wants it", () => {
    expect(names("docker run --na")).toContain("--name");
    // --name takes a value: what follows it is the value, so no options here.
    expect(names("docker run --name web --r")).toContain("--restart");
    expect(names("docker run --name ")).toEqual([]);
  });

  it("splits --opt=value and offers the allowed values", () => {
    expect(names("docker build --progress=")).toEqual(["auto", "plain", "tty"]);
    expect(inserts("docker build --progress=pl")).toEqual(["--progress=plain "]);
    expect(names("docker build --progress ")).toEqual(["auto", "plain", "tty"]);
  });
});

describe("kubectl", () => {
  it("lists verbs and their options", () => {
    expect(names("kubectl ge")).toContain("get");
    expect(names("kubectl get pods --out")).toContain("--output");
  });

  it("completes the global options anywhere", () => {
    expect(names("kubectl get pods --names")).toContain("--namespace");
  });
});

describe("systemctl", () => {
  it("lists the verbs", () => {
    expect(names("systemctl res")).toEqual(expect.arrayContaining(["restart"]));
    expect(names("systemctl st")).toEqual(expect.arrayContaining(["status", "start", "stop"]));
  });

  it("works behind sudo, with its own options first", () => {
    expect(names("sudo systemctl res")).toContain("restart");
    expect(names("sudo -u root systemctl res")).toContain("restart");
    expect(names("sudo -")).toEqual(expect.arrayContaining(["-u", "--user"]));
  });

  it("works behind env and its assignments", () => {
    expect(names("env FOO=1 systemctl res")).toContain("restart");
  });
});

describe("tar and short options", () => {
  it("knows -xzf is -x -z -f, with f taking the next word as its value", () => {
    const w = walk(["tar", "-xzf"], specs);
    expect(w.pending).not.toBeNull();
    expect(walk(["tar", "-xzf", "a.tgz"], specs).pending).toBeNull();
  });

  it("takes a value glued to its letter", () => {
    expect(walk(["tar", "-xzfa.tgz"], specs).pending).toBeNull();
  });

  it("does not offer an option already given in a bundle", () => {
    expect(names("tar -xzf a.tgz --verb")).toContain("--verbose");
  });

  it("asks the host for files where a file goes", () => {
    expect(completeLine("tar -xzf ", specs).expects).toBe("file");
    expect(completeLine("tar -czf out.tgz ", specs).expects).toBeDefined();
  });
});

describe("command names and the rest", () => {
  it("completes the first word from the bundled commands", () => {
    expect(names("kubec")).toEqual(["kubectl"]);
    expect(names("doc")).toEqual(expect.arrayContaining(["docker"]));
    expect(names("")).toEqual([]);
  });

  it("completes the command after a pipe, and after sudo", () => {
    expect(names("cat f | gre")).toContain("grep");
    expect(names("sudo syst")).toContain("systemctl");
  });

  it("says when a spec still has to be loaded, and is quiet about commands it has none for", () => {
    const lazy: Specs = { known: () => true, get: () => undefined, names: () => [] };
    expect(completeLine("git ch", lazy)).toMatchObject({ items: [], missing: "git" });
    expect(completeLine("mytool ch", specs)).toMatchObject({ items: [] });
  });

  it("stops treating words as options after --", () => {
    expect(names("git checkout -- --am")).toEqual([]);
  });

  it("quotes a value that has a space", () => {
    const odd: Specs = { known: () => true, get: () => ({ a: [{ k: "enum", v: ["a b"] }] }), names: () => [] };
    expect(completeLine("x ", odd).items[0].insert).toBe("'a b' ");
  });

  it("finds commands by their path", () => {
    expect(names("/usr/bin/git ch")).toContain("checkout");
  });
});

describe("what the host is asked about", () => {
  const at = (line: string) => completeLine(line, specs);

  it("sees a path in the word, for any command, and what the spec says a file is", () => {
    expect(at("cat /etc/ho").path).toBe("file");
    expect(at("cat ~/").path).toBe("file");
    expect(at("cat ./a").path).toBe("file");
    expect(at("mytool src/ma").path).toBe("file");
    expect(at("cat ").path).toBe("file");
    expect(at("tar -xzf a.tgz -C ").path).toBe("dir");
    expect(at("ls > ").path).toBe("file");
    expect(at("git checkout ").path).toBeUndefined();
    expect(at("mytool ab").path).toBeUndefined();
  });

  it("hands over the word as typed and unquoted, and after --opt= only the part after the sign", () => {
    expect(at("cat my\\ do").word).toEqual({ raw: "my\\ do", value: "my do" });
    expect(at("curl --output=/tmp/o").word).toEqual({ raw: "/tmp/o", value: "/tmp/o" });
    expect(at("curl --output=/tmp/o").path).toBe("file");
  });

  it("is not a path inside an option or a quote", () => {
    expect(at("tar -").path).toBeUndefined();
    expect(at("cat '/etc/ho").path).toBeUndefined();
  });

  it("names the read-only lookup that fits", () => {
    expect(at("git checkout ").generator).toBe("git-branches");
    expect(at("git switch fea").generator).toBe("git-branches");
    expect(at("systemctl restart ").generator).toBe("systemd-units");
    expect(at("sudo systemctl status ng").generator).toBe("systemd-units");
    expect(at("docker logs ").generator).toBe("docker-containers");
    expect(at("docker rmi ").generator).toBe("docker-images");
    expect(at("kubectl get pods -n ").generator).toBe("kubectl-namespaces");
    expect(at("kubectl logs ").generator).toBe("kubectl-pods");
  });

  it("has no lookup where a name is not what goes", () => {
    expect(at("git commit ").generator).toBeUndefined();
    expect(at("docker logs web ").generator).toBeUndefined();
    expect(at("git checkout -- ").generator).toBeUndefined();
    expect(at("ls ").generator).toBeUndefined();
  });
});
