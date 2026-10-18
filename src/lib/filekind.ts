// A coarse kind for a file, to pick its icon in the file manager. By name only: a listing has no content
// to sniff, and the extension is what a person scans for anyway.

export type FileKind = "folder" | "code" | "archive" | "config" | "key" | "image" | "text" | "file";

const CODE = new Set("js mjs cjs ts tsx jsx svelte vue py rb php go rs c h cc cpp hpp cs java kt swift sh bash zsh fish ps1 lua pl sql html css scss less".split(" "));
const ARCHIVE = new Set("zip tar gz tgz bz2 xz zst 7z rar jar war deb rpm iso dmg".split(" "));
const CONFIG = new Set("conf cfg ini yaml yml toml json json5 xml env properties service timer socket".split(" "));
const KEY = new Set("pem key pub crt cer csr p12 pfx gpg asc ppk kdbx".split(" "));
const IMAGE = new Set("png jpg jpeg gif webp svg bmp ico tif tiff avif".split(" "));
const TEXT = new Set("txt md markdown rst log csv tsv".split(" "));

/** Names that are a key or a config file whatever their extension (or lack of one). */
const KEY_NAMES = /^(id_(rsa|dsa|ecdsa|ed25519)(\.pub)?|authorized_keys|known_hosts|.*_rsa|.*_ed25519)$/;
const CONFIG_NAMES = /^(\.?(bashrc|zshrc|profile|bash_profile|vimrc|gitconfig|npmrc|env(\..+)?)|dockerfile|makefile|crontab|fstab|hosts|passwd|sudoers|sshd_config|ssh_config|nginx\.conf)$/i;

export function fileKind(name: string, isDir: boolean): FileKind {
  if (isDir) return "folder";
  if (KEY_NAMES.test(name)) return "key";
  if (CONFIG_NAMES.test(name)) return "config";
  const dot = name.lastIndexOf(".");
  // ".bashrc" is a name, not an extension; "a.tar.gz" is judged by its last part.
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
  if (KEY.has(ext)) return "key";
  if (ARCHIVE.has(ext)) return "archive";
  if (CONFIG.has(ext)) return "config";
  if (CODE.has(ext)) return "code";
  if (IMAGE.has(ext)) return "image";
  if (TEXT.has(ext)) return "text";
  return "file";
}
