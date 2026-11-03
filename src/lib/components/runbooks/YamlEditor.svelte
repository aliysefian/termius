<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { acceptCompletion, autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap, type CompletionContext } from "@codemirror/autocomplete";
  import { defaultKeymap, history, historyKeymap, indentWithTab, toggleComment } from "@codemirror/commands";
  import { bracketMatching, HighlightStyle, indentOnInput, syntaxHighlighting } from "@codemirror/language";
  import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
  import { highlightSelectionMatches, openSearchPanel, search, searchKeymap } from "@codemirror/search";
  import { EditorState } from "@codemirror/state";
  import { drawSelection, dropCursor, EditorView, highlightActiveLine, highlightActiveLineGutter, highlightSpecialChars, keymap, lineNumbers, tooltips } from "@codemirror/view";
  import { yaml } from "@codemirror/lang-yaml";
  import { json } from "@codemirror/lang-json";
  import { tags } from "@lezer/highlight";
  import { formatOf } from "$lib/runbookdoc";
  import { keysFor, valuesFor, yamlContext } from "$lib/runbookschema";
  import type { Problem } from "$lib/runbook";

  let {
    value,
    problems,
    parseError,
    stepLines,
    rollbackLines,
    onchange,
    onsave,
    onformat,
    onvalidate,
  }: {
    value: string;
    problems: Problem[];
    parseError: { message: string; line: number | null; column: number | null } | null;
    /** The 1-based line each step and rollback step starts on, to put a problem about a step on its line. */
    stepLines: number[];
    rollbackLines: number[];
    onchange: (v: string) => void;
    onsave: () => void;
    onformat: () => void;
    onvalidate: () => void;
  } = $props();

  let host = $state<HTMLDivElement>();
  let view: EditorView | null = null;

  const colors = HighlightStyle.define([
    { tag: [tags.propertyName, tags.definition(tags.propertyName)], color: "var(--color-accent)" },
    { tag: [tags.string, tags.special(tags.string)], color: "var(--color-success)" },
    { tag: [tags.number, tags.bool, tags.null], color: "var(--color-warning)" },
    { tag: [tags.lineComment, tags.blockComment, tags.comment], color: "var(--color-fg-muted)", fontStyle: "italic" },
    { tag: [tags.meta, tags.punctuation, tags.separator], color: "var(--color-fg-muted)" },
  ]);

  const theme = EditorView.theme({
    "&": { height: "100%", color: "var(--color-fg)", backgroundColor: "var(--color-base)" },
    "&.cm-focused": { outline: "2px solid var(--color-accent)", outlineOffset: "-2px" },
    ".cm-scroller": { overflow: "auto", fontFamily: "var(--font-mono)", fontSize: "0.8rem", lineHeight: "1.6" },
    ".cm-content": { padding: "0.75rem 0", caretColor: "var(--color-fg)" },
    ".cm-gutters": { backgroundColor: "var(--color-base)", color: "var(--color-fg-muted)", border: "none", borderRight: "1px solid var(--color-line)" },
    ".cm-lineNumbers .cm-gutterElement": { padding: "0 0.5rem 0 0.75rem", minWidth: "2.5rem", fontSize: "0.7rem" },
    ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--color-panel-hover) 50%, transparent)" },
    ".cm-activeLineGutter": { backgroundColor: "color-mix(in srgb, var(--color-panel-hover) 50%, transparent)", color: "var(--color-fg)" },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection": { backgroundColor: "color-mix(in srgb, var(--color-accent) 30%, transparent) !important" },
    ".cm-tooltip": { border: "1px solid var(--color-line)", backgroundColor: "var(--color-panel)", color: "var(--color-fg)" },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": { backgroundColor: "var(--color-accent-solid)", color: "white" },
    ".cm-diagnostic": { padding: "0.25rem 0.5rem", fontSize: "0.75rem" },
    ".cm-diagnostic-error": { borderLeft: "3px solid var(--color-danger)" },
    ".cm-lintRange-error": { backgroundImage: "none", textDecoration: "underline wavy var(--color-danger)" },
    ".cm-panels": { backgroundColor: "var(--color-panel)", color: "var(--color-fg)" },
    ".cm-searchMatch": { backgroundColor: "color-mix(in srgb, var(--color-warning) 30%, transparent)" },
  });

  /** Keys and values the schema knows, where the cursor is. YAML only. */
  function complete(ctx: CompletionContext) {
    if (formatOf(ctx.state.doc.toString()) === "json") return null;
    const word = ctx.matchBefore(/[\w./:-]*/);
    const c = yamlContext(ctx.state.doc.toString(), ctx.pos);
    if (c.valueOf) {
      const options = valuesFor(c.path, c.valueOf);
      if (!options.length || (!ctx.explicit && !c.prefix)) return null;
      return { from: ctx.pos - c.prefix.length, options: options.filter((o) => o.startsWith(c.prefix)).map((label) => ({ label, type: "enum" })) };
    }
    if (!word || (word.from === word.to && !ctx.explicit)) return null;
    const keys = keysFor(c.path);
    if (!keys.length) return null;
    return { from: word.from, options: keys.filter((k) => k.startsWith(c.prefix)).map((label) => ({ label: `${label}`, apply: `${label}: `, type: "property" })) };
  }

  const language = () => (formatOf(value) === "json" ? json() : yaml());

  function extensions() {
    return [
      lineNumbers(),
      highlightActiveLineGutter(),
      highlightSpecialChars(),
      history(),
      drawSelection(),
      dropCursor(),
      indentOnInput(),
      bracketMatching(),
      closeBrackets(),
      highlightActiveLine(),
      search({ top: true }),
      highlightSelectionMatches(),
      tooltips({ position: "fixed" }),
      syntaxHighlighting(colors),
      theme,
      EditorState.tabSize.of(2),
      EditorView.contentAttributes.of({ "aria-label": "Runbook document", spellcheck: "false", autocomplete: "off" }),
      language(),
      autocompletion({ override: [complete], activateOnTyping: true }),
      keymap.of([
        { key: "Mod-s", run: () => (onsave(), true), preventDefault: true },
        { key: "Mod-Enter", run: () => (onvalidate(), true), preventDefault: true },
        { key: "Mod-Shift-F", run: () => (onformat(), true), preventDefault: true },
        { key: "Mod-/", run: toggleComment },
        { key: "Mod-f", run: openSearchPanel, preventDefault: true },
        { key: "Tab", run: acceptCompletion },
        ...searchKeymap,
        ...closeBracketsKeymap,
        ...completionKeymap,
        indentWithTab,
        ...defaultKeymap,
        ...historyKeymap,
      ]),
      EditorView.updateListener.of((u) => {
        if (u.docChanged) onchange(u.state.doc.toString());
      }),
    ];
  }

  /** The problems as marks on the lines they belong to. A problem about a step is put on the step's first line. */
  function diagnostics(state: EditorState): Diagnostic[] {
    const out: Diagnostic[] = [];
    const doc = state.doc;
    const line = (n: number | null | undefined) => doc.line(Math.min(doc.lines, Math.max(1, n ?? 1)));
    if (parseError) {
      const l = line(parseError.line);
      const from = Math.min(l.to, l.from + Math.max(0, (parseError.column ?? 1) - 1));
      out.push({ from, to: Math.max(from, l.to), severity: "error", message: `Line ${l.number}: ${parseError.message}` });
      return out;
    }
    for (const p of problems) {
      const rollback = p.message.startsWith("Rollback step ");
      const n = rollback ? Number(/^Rollback step (\d+)/.exec(p.message)?.[1]) - 1 : p.step;
      const at = rollback ? rollbackLines[n as number] : n !== null && n !== undefined ? stepLines[n] : (p.line ?? 1);
      const l = line(at);
      out.push({ from: l.from, to: l.to, severity: "error", message: `Line ${l.number}: ${rollback || p.step !== null ? `Step ${(n ?? 0) + 1}: ` : ""}${p.message}` });
    }
    return out;
  }

  onMount(() => {
    view = new EditorView({ state: EditorState.create({ doc: value, extensions: extensions() }), parent: host });
  });

  // The text changed from outside (the Builder): put it in, keeping the cursor where it can be kept.
  $effect(() => {
    const v = value;
    if (!view || view.state.doc.toString() === v) return;
    const sel = view.state.selection.main;
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: v }, selection: { anchor: Math.min(sel.anchor, v.length) } });
  });

  $effect(() => {
    void problems;
    void parseError;
    void stepLines;
    void rollbackLines;
    void value;
    if (view) view.dispatch(setDiagnostics(view.state, diagnostics(view.state)));
  });

  onDestroy(() => {
    view?.destroy();
    view = null;
  });

  export function focus() {
    view?.focus();
  }

  export function goTo(line: number) {
    if (!view) return;
    const l = view.state.doc.line(Math.min(view.state.doc.lines, Math.max(1, line)));
    view.dispatch({ selection: { anchor: l.from }, scrollIntoView: true });
    view.focus();
  }
</script>

<div bind:this={host} class="h-full min-h-40 overflow-hidden" data-testid="yaml-editor"></div>
