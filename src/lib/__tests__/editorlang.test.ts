import { describe, expect, it } from "vitest";
import { languageFor, languageForContent } from "$lib/editorlang";

describe("languageFor", () => {
  it.each([
    ["/etc/nginx/conf.d/subdomains.conf", "nginx"],
    ["/etc/nginx/nginx.conf", "nginx"],
    ["/etc/nginx/sites-available/default", "nginx"],
    ["/etc/other/sites-available/default", "plain"],
    ["/srv/app/docker-compose.yml", "yaml"],
    ["/srv/app/values.yaml", "yaml"],
    ["/srv/app/package.json", "json"],
    ["/srv/app/tsconfig.jsonc", "json"],
    ["/srv/app/Cargo.toml", "toml"],
    ["/srv/app/pyproject.toml", "toml"],
    ["/etc/systemd/system/app.service", "systemd"],
    ["/etc/systemd/system/app.timer", "systemd"],
    ["/etc/systemd/resolved.conf", "systemd"],
    ["/home/u/.bashrc", "shell"],
    ["/home/u/deploy.sh", "shell"],
    ["/srv/app/Dockerfile", "dockerfile"],
    ["/srv/app/Dockerfile.prod", "dockerfile"],
    ["/srv/app/app.dockerfile", "dockerfile"],
    ["/srv/app/.env", "env"],
    ["/srv/app/.env.production", "env"],
    ["/srv/app/prod.env", "env"],
    ["/etc/ssh/sshd_config", "plain"],
    ["/etc/php/8.2/php.ini", "ini"],
    ["/etc/mysql/my.cnf", "ini"],
    ["/etc/app/app.conf", "ini"],
    ["/srv/notes.txt", "plain"],
    ["C:\\Users\\me\\Dockerfile", "dockerfile"],
  ])("%s is %s", (path, id) => expect(languageFor(path)).toBe(id));

  it("is not fooled by case", () => {
    expect(languageFor("/srv/DOCKERFILE")).toBe("dockerfile");
    expect(languageFor("/srv/App.YML")).toBe("yaml");
  });
});

describe("languageForContent", () => {
  it("takes a script with no telling name from its first line", () => {
    expect(languageForContent("/usr/local/bin/backup", "#!/bin/bash\nset -e\n")).toBe("shell");
    expect(languageForContent("/usr/local/bin/backup", "#!/usr/bin/env sh\n")).toBe("shell");
    expect(languageForContent("/usr/local/bin/tool", "#!/usr/bin/python3\n")).toBe("plain");
    expect(languageForContent("/srv/app/x.yml", "#!/bin/bash\n")).toBe("yaml");
  });
});
