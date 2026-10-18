// What the remote editor is made of: the highlighter for a syntax (loaded when needed, so the editor's weight is not
// in the app's start-up), and the editor's look, which uses the app's own colour tokens so every theme works.

import { HighlightStyle, StreamLanguage, syntaxHighlighting } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import type { LangId } from "$lib/editorlang";

/** The highlighter for `id`, or none for plain text. */
export async function languageExtension(id: LangId): Promise<Extension> {
  switch (id) {
    case "nginx":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/nginx")).nginx);
    case "yaml":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/yaml")).yaml);
    case "json":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/javascript")).json);
    case "toml":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/toml")).toml);
    case "shell":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/shell")).shell);
    case "dockerfile":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/dockerfile")).dockerFile);
    // `[Section]` headers and `key=value` lines: unit files, .env files and ini-style settings share one shape.
    case "systemd":
    case "env":
    case "ini":
      return StreamLanguage.define((await import("@codemirror/legacy-modes/mode/properties")).properties);
    default:
      return [];
  }
}

const colors = HighlightStyle.define([
  { tag: [tags.keyword, tags.operatorKeyword, tags.modifier], color: "var(--color-accent)", fontWeight: "600" },
  { tag: [tags.string, tags.special(tags.string)], color: "var(--color-success)" },
  { tag: [tags.number, tags.bool, tags.null, tags.atom], color: "var(--color-warning)" },
  { tag: [tags.lineComment, tags.blockComment, tags.comment], color: "var(--color-fg-muted)", fontStyle: "italic" },
  { tag: [tags.propertyName, tags.attributeName, tags.definition(tags.variableName)], color: "var(--color-fg)", fontWeight: "600" },
  { tag: [tags.typeName, tags.className, tags.heading, tags.meta, tags.labelName], color: "var(--color-warning)" },
  { tag: [tags.variableName, tags.special(tags.variableName)], color: "var(--color-accent)" },
  { tag: [tags.operator, tags.punctuation, tags.separator, tags.bracket], color: "var(--color-fg-muted)" },
]);

const theme = EditorView.theme({
  "&": { height: "100%", color: "var(--color-fg)", backgroundColor: "var(--color-base)", fontSize: "12.5px" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.55", overflow: "auto" },
  ".cm-content": { caretColor: "var(--color-fg)", padding: "8px 0" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--color-fg)" },
  ".cm-gutters": { backgroundColor: "var(--color-base)", color: "var(--color-fg-muted)", border: "none", borderRight: "1px solid var(--color-line)" },
  ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--color-fg) 5%, transparent)" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--color-fg)" },
  ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection": { backgroundColor: "color-mix(in srgb, var(--color-accent) 30%, transparent)" },
  ".cm-matchingBracket": { backgroundColor: "color-mix(in srgb, var(--color-accent) 25%, transparent)", outline: "none" },
  ".cm-searchMatch": { backgroundColor: "color-mix(in srgb, var(--color-warning) 30%, transparent)" },
  ".cm-panels": { backgroundColor: "var(--color-panel)", color: "var(--color-fg)", borderColor: "var(--color-line)" },
  ".cm-panels input, .cm-panels button": { color: "var(--color-fg)", backgroundColor: "var(--color-base)", border: "1px solid var(--color-line)", borderRadius: "4px", padding: "1px 6px" },
});

/** The editor's look: colours from the theme tokens, and highlighting by them. */
export const editorLook: Extension = [theme, syntaxHighlighting(colors)];
