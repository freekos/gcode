<script lang="ts" module>
  // Tiny safe markdown for agent messages: headings, lists, fenced code,
  // bold/inline-code. Segment-based rendering — no raw HTML injection.
  export type MdBlock =
    | { kind: "code"; lang: string; text: string }
    | { kind: "h"; level: number; text: string }
    | { kind: "ul"; items: string[] }
    | { kind: "ol"; items: string[] }
    | { kind: "p"; text: string };

  export function parseMd(src: string): MdBlock[] {
    const out: MdBlock[] = [];
    const lines = src.split("\n");
    let i = 0;
    while (i < lines.length) {
      const line = lines[i];
      if (line.startsWith("```")) {
        const lang = line.slice(3).trim();
        const buf: string[] = [];
        i++;
        while (i < lines.length && !lines[i].startsWith("```")) {
          buf.push(lines[i]);
          i++;
        }
        i++; // closing fence (or EOF while streaming)
        out.push({ kind: "code", lang, text: buf.join("\n") });
        continue;
      }
      const h = line.match(/^(#{1,4})\s+(.*)$/);
      if (h) {
        out.push({ kind: "h", level: h[1].length, text: h[2] });
        i++;
        continue;
      }
      if (/^\s*[-*]\s+/.test(line)) {
        const items: string[] = [];
        while (i < lines.length && /^\s*[-*]\s+/.test(lines[i])) {
          items.push(lines[i].replace(/^\s*[-*]\s+/, ""));
          i++;
        }
        out.push({ kind: "ul", items });
        continue;
      }
      if (/^\s*\d+[.)]\s+/.test(line)) {
        const items: string[] = [];
        while (i < lines.length && /^\s*\d+[.)]\s+/.test(lines[i])) {
          items.push(lines[i].replace(/^\s*\d+[.)]\s+/, ""));
          i++;
        }
        out.push({ kind: "ol", items });
        continue;
      }
      if (line.trim() === "") {
        i++;
        continue;
      }
      const buf: string[] = [line];
      i++;
      while (
        i < lines.length &&
        lines[i].trim() !== "" &&
        !lines[i].startsWith("```") &&
        !/^(#{1,4})\s+/.test(lines[i]) &&
        !/^\s*[-*]\s+/.test(lines[i]) &&
        !/^\s*\d+[.)]\s+/.test(lines[i])
      ) {
        buf.push(lines[i]);
        i++;
      }
      out.push({ kind: "p", text: buf.join("\n") });
    }
    return out;
  }

  export type Seg = { t: string; code?: boolean; bold?: boolean };
  export function inlineSegs(text: string): Seg[] {
    return text
      .split(/(`[^`\n]+`|\*\*[^*\n]+\*\*)/g)
      .filter(Boolean)
      .map((part) => {
        if (part.startsWith("`") && part.endsWith("`")) return { t: part.slice(1, -1), code: true };
        if (part.startsWith("**") && part.endsWith("**")) return { t: part.slice(2, -2), bold: true };
        return { t: part };
      });
  }
</script>

<script lang="ts">
  let { text }: { text: string } = $props();
  const blocks = $derived(parseMd(text));
</script>

<div class="md">
  {#each blocks as b, i (i)}
    {#if b.kind === "code"}
      <pre class="md-code"><code>{b.text}</code></pre>
    {:else if b.kind === "h"}
      <div class="md-h md-h{Math.min(b.level, 3)}">{#each inlineSegs(b.text) as s, j (j)}{#if s.code}<code class="md-ic">{s.t}</code>{:else}{s.t}{/if}{/each}</div>
    {:else if b.kind === "ul"}
      <ul class="md-list">
        {#each b.items as it, j (j)}<li>{#each inlineSegs(it) as s, k (k)}{#if s.code}<code class="md-ic">{s.t}</code>{:else if s.bold}<b>{s.t}</b>{:else}{s.t}{/if}{/each}</li>{/each}
      </ul>
    {:else if b.kind === "ol"}
      <ol class="md-list">
        {#each b.items as it, j (j)}<li>{#each inlineSegs(it) as s, k (k)}{#if s.code}<code class="md-ic">{s.t}</code>{:else if s.bold}<b>{s.t}</b>{:else}{s.t}{/if}{/each}</li>{/each}
      </ol>
    {:else}
      <p class="md-p">{#each inlineSegs(b.text) as s, j (j)}{#if s.code}<code class="md-ic">{s.t}</code>{:else if s.bold}<b>{s.t}</b>{:else}{s.t}{/if}{/each}</p>
    {/if}
  {/each}
</div>

<style>
  .md { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .md-p { margin: 0; white-space: pre-wrap; }
  .md-h { font-weight: 650; color: var(--text-primary); }
  .md-h1 { font-size: 15px; margin-top: 6px; }
  .md-h2 { font-size: 14px; margin-top: 4px; }
  .md-h3 { font-size: 13.5px; }
  .md-list { margin: 0; padding-left: 20px; display: flex; flex-direction: column; gap: 3px; }
  .md-code {
    background: var(--surface-0);
    border: 1px solid var(--border-subtle);
    border-radius: var(--r-md);
    padding: 10px 12px;
    overflow-x: auto;
    margin: 0;
  }
  .md-code code { font: 11.5px var(--font-mono); color: var(--text-secondary); }
  .md-ic {
    font-family: var(--font-mono);
    font-size: 11.5px;
    background: var(--surface-2);
    border-radius: 6px;
    padding: 1px 7px;
  }
  b { color: var(--text-primary); font-weight: 600; }
</style>
