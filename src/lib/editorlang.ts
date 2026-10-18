// Which syntax a file in the remote editor is highlighted as, from its name (and, for a script with no extension,
// its first line). Names only decide here; the highlighters themselves are loaded when the editor opens.

export type LangId = "nginx" | "yaml" | "json" | "toml" | "systemd" | "shell" | "dockerfile" | "env" | "ini" | "plain";

const lower = (s: string) => s.toLowerCase();

export function baseName(path: string) {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? path;
}

/** The syntax for a file path. `.conf` is nginx under an nginx folder (or called nginx…), and plain settings elsewhere. */
export function languageFor(path: string): LangId {
  const name = lower(baseName(path));
  const full = lower(path);
  if (name === "dockerfile" || name.startsWith("dockerfile.") || name.endsWith(".dockerfile") || name === "containerfile") return "dockerfile";
  if (name === ".env" || name.startsWith(".env.") || name.endsWith(".env")) return "env";
  if (/^docker-compose.*\.ya?ml$/.test(name) || /\.ya?ml$/.test(name)) return "yaml";
  if (/\.(json|jsonc|json5|webmanifest)$/.test(name) || name === ".babelrc" || name === ".eslintrc") return "json";
  if (/\.toml$/.test(name) || name === "cargo.lock" || name === "poetry.lock") return "toml";
  if (/\.(service|socket|timer|mount|automount|target|path|slice|scope|swap|network|netdev|link)$/.test(name)) return "systemd";
  if (/\.(sh|bash|zsh|ksh|fish)$/.test(name) || /^\.?(bashrc|zshrc|profile|bash_profile|bash_aliases|bash_logout|zprofile|zshenv|kshrc)$/.test(name)) return "shell";
  if (/\.conf$/.test(name) || /\.nginx$/.test(name)) {
    if (/(^|\/)nginx(\/|$)/.test(full) || name.startsWith("nginx") || /\.nginx$/.test(name) || /(^|\/)(sites-available|sites-enabled|conf\.d)\//.test(full) && /nginx/.test(full)) return "nginx";
    if (/(^|\/)systemd(\/|$)/.test(full)) return "systemd";
    return "ini";
  }
  if (/\.(ini|cfg|properties|desktop|repo|editorconfig|gitconfig)$/.test(name) || name === "my.cnf") return "ini";
  // sites-available/default and the like: no extension, but plainly nginx by where they live.
  if (!name.includes(".") && /(^|\/)nginx\//.test(full)) return "nginx";
  return "plain";
}

/** A script with no telling name is still a script if it says so on its first line. */
export function languageForContent(path: string, text: string): LangId {
  const byName = languageFor(path);
  if (byName !== "plain") return byName;
  const first = text.slice(0, 200).split("\n", 1)[0];
  if (/^#!\s*\/(usr\/)?(local\/)?bin\/(env\s+)?(ba|z|k|da|a)?sh\b/.test(first)) return "shell";
  return "plain";
}

/** A short name for the status line. */
export const LANGUAGE_LABEL: Record<LangId, string> = {
  nginx: "nginx",
  yaml: "YAML",
  json: "JSON",
  toml: "TOML",
  systemd: "systemd unit",
  shell: "shell",
  dockerfile: "Dockerfile",
  env: ".env",
  ini: "settings",
  plain: "plain text",
};
