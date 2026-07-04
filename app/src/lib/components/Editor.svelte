<script lang="ts">
  import { onMount } from "svelte";
  import { EditorView, keymap, lineNumbers, highlightActiveLine } from "@codemirror/view";
  import { EditorState } from "@codemirror/state";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { syntaxHighlighting, HighlightStyle, bracketMatching } from "@codemirror/language";
  import { languages } from "@codemirror/language-data";
  import { tags } from "@lezer/highlight";

  // CodeMirror 6 editor themed with our tokens (phase 4 decision).
  let {
    content,
    path,
    onsave,
    onquote,
  }: {
    content: string;
    path: string; // used for language detection
    onsave: (text: string) => void;
    onquote?: (from: number, to: number, code: string) => void;
  } = $props();

  let host: HTMLElement;
  let view: EditorView | undefined;
  let dirty = $state(false);

  const theme = EditorView.theme(
    {
      "&": { backgroundColor: "transparent", fontSize: "12.5px", height: "100%" },
      ".cm-content": { fontFamily: "var(--font-mono)", caretColor: "var(--text-primary)" },
      ".cm-gutters": {
        backgroundColor: "transparent",
        color: "var(--text-disabled)",
        border: "none",
      },
      ".cm-activeLine": { backgroundColor: "oklch(100% 0 0 / 0.035)" },
      ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--text-muted)" },
      "&.cm-focused": { outline: "none" },
      ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
        backgroundColor: "var(--accent-soft) !important",
      },
      ".cm-cursor": { borderLeftColor: "var(--text-primary)" },
    },
    { dark: true },
  );

  const hl = HighlightStyle.define([
    { tag: tags.keyword, color: "oklch(72% 0.09 300)" },
    { tag: tags.string, color: "oklch(74% 0.09 140)" },
    { tag: tags.comment, color: "var(--text-muted)", fontStyle: "italic" },
    { tag: tags.function(tags.variableName), color: "oklch(74% 0.08 250)" },
    { tag: tags.number, color: "oklch(76% 0.1 60)" },
    { tag: tags.typeName, color: "oklch(76% 0.07 200)" },
    { tag: tags.propertyName, color: "oklch(78% 0.05 250)" },
  ]);

  async function langFor(p: string) {
    const desc = languages.find((l) => l.extensions.some((e) => p.endsWith(`.${e}`)));
    if (!desc) return [];
    const support = await desc.load();
    return [support];
  }

  onMount(() => {
    let destroyed = false;
    (async () => {
      const lang = await langFor(path);
      if (destroyed) return;
      view = new EditorView({
        parent: host,
        state: EditorState.create({
          doc: content,
          extensions: [
            lineNumbers(),
            history(),
            highlightActiveLine(),
            bracketMatching(),
            keymap.of([
              {
                key: "Mod-s",
                run: (v: EditorView) => {
                  onsave(v.state.doc.toString());
                  dirty = false;
                  return true;
                },
              },
              {
                // Cursor-style cmd-L: quote the selection into the agent composer
                key: "Mod-l",
                run: (v: EditorView) => {
                  const sel = v.state.selection.main;
                  if (sel.empty || !onquote) return false;
                  const from = v.state.doc.lineAt(sel.from).number;
                  const to = v.state.doc.lineAt(sel.to).number;
                  onquote(from, to, v.state.sliceDoc(sel.from, sel.to));
                  return true;
                },
              },
              indentWithTab,
              ...defaultKeymap,
              ...historyKeymap,
            ]),
            theme,
            syntaxHighlighting(hl),
            EditorView.updateListener.of((u: { docChanged: boolean }) => {
              if (u.docChanged) dirty = true;
            }),
            ...lang,
          ],
        }),
      });
    })();
    return () => {
      destroyed = true;
      view?.destroy();
    };
  });

  export function save() {
    if (view) {
      onsave(view.state.doc.toString());
      dirty = false;
    }
  }
  export function isDirty() {
    return dirty;
  }
</script>

<div class="ed-wrap">
  <div class="ed-bar">
    <span class="mono">{path}</span>
    {#if dirty}<span class="dot" data-tip="Есть несохранённые правки · ⌘S" aria-label="Не сохранено"></span>{/if}
    <span class="grow"></span>
    <span class="hint">⌘S — сохранить{#if onquote} · ⌘L — в промпт{/if}</span>
  </div>
  <div class="ed-host" bind:this={host}></div>
</div>

<style>
  .ed-wrap { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .ed-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 96px 6px 14px; /* keep clear of the card action buttons */
    color: var(--text-secondary);
  }
  .mono { font: 500 12px var(--font-mono); }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--status-review); }
  .grow { flex: 1; }
  .hint { font-size: 11px; color: var(--text-disabled); }
  .ed-host { flex: 1; overflow: auto; min-height: 0; }
  .ed-host :global(.cm-editor) { height: 100%; }
</style>
