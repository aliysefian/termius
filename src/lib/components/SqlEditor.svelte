<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { acceptCompletion, autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from "@codemirror/autocomplete";
  import { defaultKeymap, history, historyKeymap, indentWithTab, toggleComment } from "@codemirror/commands";
  import { highlightSelectionMatches, openSearchPanel, search, searchKeymap } from "@codemirror/search";
  import { bracketMatching, HighlightStyle, indentOnInput, syntaxHighlighting } from "@codemirror/language";
  import { Compartment, EditorState, type Extension, type StateEffect } from "@codemirror/state";
  import { drawSelection, dropCursor, EditorView, highlightActiveLine, highlightActiveLineGutter, highlightSpecialChars, keymap, lineNumbers, placeholder as editorPlaceholder, tooltips } from "@codemirror/view";
  import { sql, keywordCompletionSource } from "@codemirror/lang-sql";
  import { tags } from "@lezer/highlight";
  import { sqlDialect, sqlMetadataCompletion, type SqlMetadataProvider } from "$lib/sqlcompletion";
  import { statementAt } from "$lib/dbstatement";

  let {
    tabId,
    value,
    engine,
    label,
    placeholder,
    disabled = false,
    metadata,
    onchange,
    onrun,
    onsave,
    onformat,
  }: {
    tabId: string;
    value: string;
    engine: string;
    label: string;
    placeholder?: string;
    disabled?: boolean;
    metadata: SqlMetadataProvider;
    onchange: (value: string) => void;
    /** `statement`: the selection or the statement at the cursor. `script`: the selection or the whole text. */
    onrun: (mode: "statement" | "script", text: string | null) => void;
    onsave?: () => void;
    onformat?: () => void;
  } = $props();

  let host = $state<HTMLDivElement>();
  let view: EditorView | null = null;
  let activeTab = "";
  let configuredEngine = "";
  let configuredLabel = "";
  let configuredPlaceholder = "";
  let configuredDisabled = false;
  let configuredMetadata: SqlMetadataProvider | null = null;
  const states = new Map<string, EditorState>();
  const language = new Compartment();
  const editable = new Compartment();
  const attributes = new Compartment();
  const placeholderConfig = new Compartment();

  const colors = HighlightStyle.define([
    { tag: tags.keyword, color: "var(--color-accent)", fontWeight: "600" },
    { tag: [tags.string, tags.special(tags.string)], color: "var(--color-success)" },
    { tag: [tags.number, tags.bool, tags.null], color: "var(--color-warning)" },
    { tag: [tags.lineComment, tags.blockComment], color: "var(--color-fg-muted)", fontStyle: "italic" },
    { tag: [tags.typeName, tags.className], color: "var(--color-warning)" },
    { tag: [tags.operatorKeyword, tags.operator], color: "var(--color-accent)" },
    { tag: tags.punctuation, color: "var(--color-fg-muted)" },
  ]);

  const editorTheme = EditorView.theme({
    "&": { height: "100%", color: "var(--color-fg)", backgroundColor: "var(--color-base)" },
    "&.cm-focused": { outline: "2px solid var(--color-accent)", outlineOffset: "-2px" },
    ".cm-scroller": { overflow: "auto", fontFamily: "var(--font-mono)" },
    ".cm-content": { minHeight: "4rem", padding: "0.75rem", caretColor: "var(--color-fg)" },
    ".cm-line": { padding: "0" },
    ".cm-gutters": { backgroundColor: "var(--color-base)", color: "var(--color-fg-muted)", border: "none", borderRight: "1px solid var(--color-line)" },
    ".cm-lineNumbers .cm-gutterElement": { padding: "0 0.5rem 0 0.75rem", minWidth: "2.25rem", fontSize: "0.7rem" },
    ".cm-activeLineGutter": { backgroundColor: "color-mix(in srgb, var(--color-panel-hover) 55%, transparent)", color: "var(--color-fg)" },
    ".cm-searchMatch": { backgroundColor: "color-mix(in srgb, var(--color-warning) 30%, transparent)" },
    ".cm-searchMatch-selected": { backgroundColor: "color-mix(in srgb, var(--color-accent) 40%, transparent)" },
    ".cm-panel.cm-search": { padding: "0.4rem 0.5rem", fontSize: "0.75rem" },
    ".cm-panel.cm-search input, .cm-panel.cm-search button": { fontSize: "0.75rem" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--color-fg)" },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection": { backgroundColor: "color-mix(in srgb, var(--color-accent) 30%, transparent) !important" },
    ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--color-panel-hover) 55%, transparent)" },
    ".cm-tooltip": { border: "1px solid var(--color-line)", backgroundColor: "var(--color-panel)", color: "var(--color-fg)" },
    ".cm-tooltip-autocomplete > ul": { maxHeight: "16rem", fontFamily: "var(--font-mono)", fontSize: "0.75rem" },
    ".cm-tooltip-autocomplete > ul > li": { padding: "0.2rem 0.5rem" },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": { backgroundColor: "var(--color-accent-solid)", color: "white" },
    ".cm-completionDetail": { color: "var(--color-fg-muted)", fontStyle: "normal", marginLeft: "0.75rem" },
    ".cm-panels": { backgroundColor: "var(--color-panel)", color: "var(--color-fg)" },
  });

  function languageExtensions(): Extension {
    const dialect = sqlDialect(engine);
    const sources = [keywordCompletionSource(dialect, true)];
    if (engine === "postgres" || engine === "mysql") sources.push(sqlMetadataCompletion(engine, metadata));
    return [sql({ dialect }), autocompletion({ override: sources, activateOnTyping: true, maxRenderedOptions: 100 })];
  }

  function baseExtensions(): Extension[] {
    return [
      lineNumbers(),
      highlightActiveLineGutter(),
      highlightSpecialChars(),
      history(),
      search({ top: true }),
      highlightSelectionMatches(),
      drawSelection(),
      dropCursor(),
      indentOnInput(),
      bracketMatching(),
      closeBrackets(),
      highlightActiveLine(),
      EditorView.lineWrapping,
      placeholderConfig.of(editorPlaceholder(placeholder ?? "")),
      tooltips({ position: "fixed" }),
      syntaxHighlighting(colors),
      editorTheme,
      language.of(languageExtensions()),
      editable.of([EditorView.editable.of(!disabled), EditorState.readOnly.of(disabled)]),
      attributes.of(EditorView.contentAttributes.of({ "aria-label": label, spellcheck: "false", autocomplete: "off" })),
      keymap.of([
        { key: "Mod-Enter", run: runStatement },
        { key: "Mod-Shift-Enter", run: runScript },
        { key: "Mod-s", run: () => (onsave ? (onsave(), true) : false), preventDefault: true },
        { key: "Mod-Shift-f", run: () => (onformat ? (onformat(), true) : false), preventDefault: true },
        { key: "Mod-/", run: toggleComment },
        { key: "Mod-f", run: openSearchPanel, preventDefault: true },
        ...searchKeymap,
        { key: "Tab", run: acceptCompletion },
        ...closeBracketsKeymap,
        ...completionKeymap,
        indentWithTab,
        ...defaultKeymap,
        ...historyKeymap,
      ]),
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) return;
        states.set(activeTab, update.state);
        onchange(update.state.doc.toString());
      }),
    ];
  }

  /** The selection, or the statement the cursor is in. */
  function runStatement(): boolean {
    const sel = selection();
    onrun("statement", sel ?? currentStatement());
    return true;
  }

  /** The selection, or the whole text, as a script. */
  function runScript(): boolean {
    onrun("script", selection());
    return true;
  }

  export function currentStatement(): string | null {
    if (!view) return null;
    return statementAt(view.state.doc.toString(), view.state.selection.main.head, engine)?.text ?? null;
  }

  /** Replace the whole text (formatting), as one undoable change. */
  export function replaceAll(text: string) {
    if (!view) return;
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
  }

  function stateFor(id: string, doc: string): EditorState {
    const saved = states.get(id);
    if (!saved) return EditorState.create({ doc, extensions: baseExtensions() });
    if (saved.doc.toString() === doc) return saved;
    return saved.update({ changes: { from: 0, to: saved.doc.length, insert: doc } }).state;
  }

  function sync() {
    if (!view) return;
    let switched = false;
    if (tabId !== activeTab) {
      if (activeTab) states.set(activeTab, view.state);
      activeTab = tabId;
      view.setState(stateFor(tabId, value));
      switched = true;
    } else if (view.state.doc.toString() !== value) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: value } });
    }
    const effects: StateEffect<unknown>[] = [];
    if (switched || engine !== configuredEngine || metadata !== configuredMetadata) {
      effects.push(language.reconfigure(languageExtensions()));
      configuredEngine = engine;
      configuredMetadata = metadata;
    }
    if (switched || disabled !== configuredDisabled) {
      effects.push(editable.reconfigure([EditorView.editable.of(!disabled), EditorState.readOnly.of(disabled)]));
      configuredDisabled = disabled;
    }
    if (switched || label !== configuredLabel) {
      effects.push(attributes.reconfigure(EditorView.contentAttributes.of({ "aria-label": label, spellcheck: "false", autocomplete: "off" })));
      configuredLabel = label;
    }
    if (switched || (placeholder ?? "") !== configuredPlaceholder) {
      effects.push(placeholderConfig.reconfigure(editorPlaceholder(placeholder ?? "")));
      configuredPlaceholder = placeholder ?? "";
    }
    if (effects.length) view.dispatch({ effects });
  }

  export function focus() {
    view?.focus();
  }

  export function selection(): string | null {
    if (!view) return null;
    const range = view.state.selection.main;
    const text = view.state.sliceDoc(range.from, range.to);
    return text.trim() ? text : null;
  }

  onMount(() => {
    activeTab = tabId;
    configuredEngine = engine;
    configuredLabel = label;
    configuredPlaceholder = placeholder ?? "";
    configuredDisabled = disabled;
    configuredMetadata = metadata;
    view = new EditorView({ state: stateFor(tabId, value), parent: host });
  });

  $effect(() => {
    void tabId;
    void value;
    void engine;
    void label;
    void placeholder;
    void disabled;
    void metadata;
    sync();
  });

  onDestroy(() => {
    if (view && activeTab) states.set(activeTab, view.state);
    view?.destroy();
    view = null;
  });
</script>

<div bind:this={host} class="h-full min-h-16 overflow-hidden bg-base text-sm" data-testid="sql-editor"></div>
