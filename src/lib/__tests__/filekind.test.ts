import { describe, expect, it } from "vitest";
import { fileKind } from "$lib/filekind";

describe("fileKind", () => {
  it("a folder is a folder whatever it is called", () => {
    expect(fileKind("backup.tar.gz", true)).toBe("folder");
  });

  it.each([
    ["main.rs", "code"],
    ["deploy.sh", "code"],
    ["backup.tar.gz", "archive"],
    ["site.zip", "archive"],
    ["nginx.conf", "config"],
    ["docker-compose.yml", "config"],
    ["Dockerfile", "config"],
    [".bashrc", "config"],
    [".env.production", "config"],
    ["server.pem", "key"],
    ["id_ed25519", "key"],
    ["id_rsa.pub", "key"],
    ["authorized_keys", "key"],
    ["logo.PNG", "image"],
    ["README.md", "text"],
    ["notes", "file"],
    ["archive.unknown", "file"],
  ])("%s is %s", (name, kind) => {
    expect(fileKind(name, false)).toBe(kind);
  });

  it("a dotfile's name is not an extension", () => {
    expect(fileKind(".zip", false)).toBe("file");
  });
});
