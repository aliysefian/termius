import { describe, expect, it } from "vitest";
import { LogBuffer } from "../ops/logs";
import { SecretMasker, isSecretName, maskSecrets } from "../ops/mask";

// The cases mirror src-tauri/src/mask.rs, so the two implementations stay in step.
describe("hiding secrets in text", () => {
  it("hides named values but keeps the names", () => {
    expect(maskSecrets("DB_PASSWORD=hunter2")).toBe("DB_PASSWORD=[hidden]");
    expect(maskSecrets("password: hunter2 and more")).toBe("password: [hidden] and more");
    expect(maskSecrets('{"api_key": "abc123", "user": "bob"}')).toBe('{"api_key": "[hidden]", "user": "bob"}');
    expect(maskSecrets("export SECRET_TOKEN='abc' && run")).toBe("export SECRET_TOKEN='[hidden]' && run");
    expect(maskSecrets("curl -H 'Authorization: Bearer abc.def.ghi' x")).toBe("curl -H 'Authorization: Bearer [hidden]' x");
    expect(maskSecrets("mysql --password=s3cret -u root")).toBe("mysql --password=[hidden] -u root");
    expect(maskSecrets("two: password=a token=b")).toBe("two: password=[hidden] token=[hidden]");
  });

  it("leaves a name without a value, and ordinary text, alone", () => {
    for (const t of ["the password was changed", "enter your password:", "token", "no secrets here", "passwords are hard", "password:", "", "üñí ✓ text"]) {
      expect(maskSecrets(t)).toBe(t);
    }
    const t = "line one\r\nline two\n\nüñí ✓ text\n";
    expect(maskSecrets(t)).toBe(t);
    expect(maskSecrets("a=1\r\npassword=x\r\n")).toBe("a=1\r\npassword=[hidden]\r\n");
  });

  it("replaces a private key block with one line, even when it arrives a line at a time", () => {
    const t = "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\nBBBB\n-----END OPENSSH PRIVATE KEY-----\nafter\n";
    expect(maskSecrets(t)).toBe("before\n[private key hidden]\nafter\n");
    expect(maskSecrets("x\n-----BEGIN RSA PRIVATE KEY-----\nAAAA\n")).toBe("x\n[private key hidden]");
    const cert = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----";
    expect(maskSecrets(cert)).toBe(cert);

    const m = new SecretMasker();
    const got = ["a", "-----BEGIN PRIVATE KEY-----", "AAAA", "-----END PRIVATE KEY-----", "b"].map((l) => m.line(l));
    expect(got).toEqual(["a", "[private key hidden]", null, null, "b"]);
  });

  it("hides credentials in URLs and well-known token shapes", () => {
    expect(maskSecrets("git clone https://bob:pa55@example.com/r.git")).toBe("git clone https://bob:[hidden]@example.com/r.git");
    expect(maskSecrets("https://example.com/a:b")).toBe("https://example.com/a:b");
    expect(maskSecrets("id AKIAIOSFODNN7EXAMPLE end")).toBe("id [hidden] end");
    expect(maskSecrets("t=ghp_abcdefghijklmnopqrstuvwxyz0123456789")).toBe("t=[hidden]");
    expect(maskSecrets("use sk-learn for models")).toBe("use sk-learn for models");
    expect(maskSecrets("key sk-abcdefghijklmnopqrstuvwxyz012345 ok")).toBe("key [hidden] ok");
  });

  it("recognises secret-looking names", () => {
    for (const n of ["password", "DB_PASSWORD", "api_key", "authToken", "client_secret"]) expect(isSecretName(n), n).toBe(true);
    for (const n of ["service", "host", "limit"]) expect(isSecretName(n), n).toBe(false);
  });
});

describe("the log buffer", () => {
  it("hides secrets as lines arrive, per host, including a key block split across chunks", () => {
    const b = new LogBuffer();
    b.add("a", "web", ["start", "-----BEGIN RSA PRIVATE KEY-----"]);
    b.add("b", "db", ["password=hunter2"]); // another host's stream is not inside a's key block
    b.add("a", "web", ["AAAA", "-----END RSA PRIVATE KEY-----", "done"]);
    expect(b.lines.map((l) => l.text)).toEqual(["start", "[private key hidden]", "password=[hidden]", "done"]);
    expect(JSON.stringify(b.lines)).not.toMatch(/hunter2|AAAA/);
  });

  it("can be told not to, and then keeps lines as they are", () => {
    const b = new LogBuffer();
    b.mask = false;
    b.add("a", "web", ["password=hunter2"]);
    expect(b.lines[0].text).toBe("password=hunter2");
  });
});
