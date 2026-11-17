import * as Dialog from "@radix-ui/react-dialog";
import { BookOpen, FileText, PlayCircle, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { docs } from "@/data/docs";
import { tutorials } from "@/tutorials/registry";
import { Badge } from "@/components/ui/badge";

interface Hit {
  url: string; // site-relative, starts with /
  title: string;
  excerpt: string;
  kind: "Docs" | "Tutorial" | "Page";
}

interface PagefindApi {
  search(q: string): Promise<{ results: { data(): Promise<{ url: string; excerpt: string; meta: { title?: string } }> }[] }>;
}

let pagefind: Promise<PagefindApi | null> | null = null;
function loadPagefind() {
  // The index only exists after `npm run build`; in dev the fallback below answers instead.
  pagefind ??= import(/* @vite-ignore */ `${import.meta.env.BASE_URL}pagefind/pagefind.js`).then((m) => m as PagefindApi).catch(() => null);
  return pagefind;
}

const strip = (u: string) => u.replace(import.meta.env.BASE_URL.replace(/\/$/, ""), "").replace(/index\.html$/, "").replace(/\.html$/, "").replace(/\/$/, "") || "/";

const esc = (v: string) => v.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function localSearch(q: string): Hit[] {
  const s = q.toLowerCase();
  const out: Hit[] = [];
  for (const d of docs) {
    const hay = `${d.title} ${d.description} ${d.headings.map((h) => h.text).join(" ")}`.toLowerCase();
    if (hay.includes(s)) out.push({ url: `/docs/${d.slug}`, title: d.title, excerpt: esc(d.description), kind: "Docs" });
  }
  for (const t of tutorials) {
    if (`${t.title} ${t.description} ${t.keywords.join(" ")}`.toLowerCase().includes(s)) out.push({ url: `/tutorials/${t.id}`, title: t.title, excerpt: esc(t.description), kind: "Tutorial" });
  }
  return out.slice(0, 12);
}

export function SearchDialog({ open, onOpenChange, returnFocus }: { open: boolean; onOpenChange: (o: boolean) => void; returnFocus?: React.RefObject<HTMLElement | null> }) {
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<Hit[]>([]);
  const [active, setActive] = useState(0);
  const navigate = useNavigate();
  const seq = useRef(0);

  useEffect(() => {
    if (!q.trim()) {
      setHits([]);
      return;
    }
    const mine = ++seq.current;
    (async () => {
      const pf = await loadPagefind();
      let found: Hit[] = [];
      if (pf) {
        const res = await pf.search(q);
        const data = await Promise.all(res.results.slice(0, 10).map((r) => r.data()));
        found = data.map((d) => {
          const url = strip(d.url);
          return { url, title: d.meta.title ?? url, excerpt: d.excerpt, kind: url.startsWith("/docs") ? "Docs" : url.startsWith("/tutorials") ? "Tutorial" : "Page" } as Hit;
        });
      }
      if (found.length === 0) found = localSearch(q);
      if (mine === seq.current) {
        setHits(found);
        setActive(0);
      }
    })();
  }, [q]);

  const go = (h: Hit) => {
    onOpenChange(false);
    setQ("");
    navigate(h.url);
  };

  const icons = useMemo(() => ({ Docs: BookOpen, Tutorial: PlayCircle, Page: FileText }), []);

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
        <Dialog.Content
          aria-describedby={undefined}
          onCloseAutoFocus={(e) => {
            if (returnFocus?.current) {
              e.preventDefault();
              returnFocus.current.focus();
            }
          }}
          className="fixed left-1/2 top-[12vh] z-50 w-[min(92vw,40rem)] -translate-x-1/2 overflow-hidden rounded-2xl border border-line bg-bg-elev shadow-2xl outline-none">
          <Dialog.Title className="sr-only">Search documentation and tutorials</Dialog.Title>
          <div className="flex items-center gap-3 border-b border-line px-4">
            <Search size={18} className="text-muted" aria-hidden="true" />
            <input
              autoFocus
              role="combobox"
              aria-expanded={hits.length > 0}
              aria-controls="search-results"
              aria-activedescendant={hits[active] ? `search-hit-${active}` : undefined}
              aria-label="Search"
              value={q}
              onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setActive((a) => Math.min(a + 1, hits.length - 1));
                } else if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setActive((a) => Math.max(a - 1, 0));
                } else if (e.key === "Enter" && hits[active]) go(hits[active]);
              }}
              placeholder="Search docs and tutorials…"
              className="h-14 w-full bg-transparent text-base text-fg outline-none placeholder:text-muted"
            />
          </div>
          <ul id="search-results" role="listbox" aria-label="Results" className="max-h-[50vh] overflow-y-auto p-2">
            {hits.map((h, i) => {
              const Icon = icons[h.kind];
              return (
                <li key={h.url + i} id={`search-hit-${i}`} role="option" aria-selected={i === active}>
                  <button type="button" onClick={() => go(h)} onMouseEnter={() => setActive(i)} className={`flex w-full items-start gap-3 rounded-lg px-3 py-2.5 text-left ${i === active ? "bg-panel-hover" : ""}`}>
                    <Icon size={16} className="mt-1 shrink-0 text-accent" aria-hidden="true" />
                    <span className="min-w-0 flex-1">
                      <span className="flex items-center gap-2 text-sm font-medium">
                        {h.title} <Badge className="px-1.5 py-0 text-[10px]">{h.kind}</Badge>
                      </span>
                      <span className="mt-0.5 line-clamp-2 text-[13px] text-muted [&_mark]:rounded [&_mark]:bg-accent/25 [&_mark]:px-0.5 [&_mark]:text-fg" dangerouslySetInnerHTML={{ __html: h.excerpt }} />
                    </span>
                  </button>
                </li>
              );
            })}
            {q.trim() && hits.length === 0 && <li className="px-3 py-6 text-center text-sm text-muted">No results for “{q}”.</li>}
            {!q.trim() && <li className="px-3 py-6 text-center text-sm text-muted">Type to search the docs and tutorials.</li>}
          </ul>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
