import { ChevronDown, ChevronRight, Menu, Pencil } from "lucide-react";
import { lazy, Suspense, useEffect, useMemo, useState } from "react";
import { MDXProvider } from "@mdx-js/react";
import { Link, Navigate, useParams } from "react-router-dom";
import { mdxComponents } from "@/components/docs/mdx";
import { Sheet } from "@/components/ui/sheet";
import { Card } from "@/components/ui/card";
import { docBySlug, docs, editUrl, loadDoc, neighbours, SECTIONS } from "@/data/docs";
import { usePageMeta } from "@/hooks/usePageMeta";
import { cn } from "@/lib/utils";
import NotFound from "./NotFound";

function DocsNav({ current, onNavigate }: { current?: string; onNavigate?: () => void }) {
  const [closed, setClosed] = useState<Record<string, boolean>>({});
  return (
    <nav aria-label="Documentation" className="space-y-5">
      {SECTIONS.map((s) => {
        const items = docs.filter((d) => d.section === s);
        if (!items.length) return null;
        const open = !closed[s] || items.some((d) => d.slug === current);
        return (
          <div key={s}>
            <button type="button" aria-expanded={open} onClick={() => setClosed((c) => ({ ...c, [s]: open }))} className="flex w-full items-center justify-between rounded-md px-2 py-1 text-xs font-semibold uppercase tracking-wider text-muted hover:text-fg">
              {s}
              {open ? <ChevronDown size={14} aria-hidden="true" /> : <ChevronRight size={14} aria-hidden="true" />}
            </button>
            {open && (
              <ul className="mt-1.5 space-y-0.5 border-l border-line-soft">
                {items.map((d) => (
                  <li key={d.slug}>
                    <Link
                      to={`/docs/${d.slug}`}
                      onClick={onNavigate}
                      aria-current={d.slug === current ? "page" : undefined}
                      className={cn("-ml-px block border-l px-4 py-1.5 text-sm transition-colors", d.slug === current ? "border-accent font-medium text-fg" : "border-transparent text-muted hover:border-line hover:text-fg")}
                    >
                      {d.title}
                    </Link>
                  </li>
                ))}
              </ul>
            )}
          </div>
        );
      })}
    </nav>
  );
}

function Toc({ slug }: { slug: string }) {
  const doc = docBySlug(slug)!;
  const [active, setActive] = useState("");
  useEffect(() => {
    const els = doc.headings.map((h) => document.getElementById(h.id)).filter((e): e is HTMLElement => !!e);
    if (!els.length || typeof IntersectionObserver === "undefined") return;
    const io = new IntersectionObserver((entries) => {
      const vis = entries.filter((e) => e.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0];
      if (vis) setActive(vis.target.id);
    }, { rootMargin: "-80px 0px -70% 0px" });
    els.forEach((e) => io.observe(e));
    return () => io.disconnect();
  }, [doc]);
  if (doc.headings.length < 2) return null;
  return (
    <nav aria-label="On this page" className="text-sm">
      <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted">On this page</h2>
      <ul className="space-y-1.5 border-l border-line-soft">
        {doc.headings.map((h) => (
          <li key={h.id}>
            <a href={`#${h.id}`} aria-current={active === h.id ? "location" : undefined} className={cn("-ml-px block border-l py-0.5 transition-colors", h.depth === 3 ? "pl-7" : "pl-4", active === h.id ? "border-accent text-fg" : "border-transparent text-muted hover:text-fg")}>
              {h.text}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}

export default function Docs() {
  const { slug } = useParams();
  if (!slug) return <DocsHome />;
  if (!docBySlug(slug)) return <NotFound />;
  return <DocPage key={slug} slug={slug} />;
}

function DocsHome() {
  usePageMeta("Documentation: SSHVault", "Guides for installing and using SSHVault: vault and sync, hosts, terminals, SFTP, tmux, tunnels, keys and troubleshooting.");
  return (
    <div className="mx-auto max-w-5xl px-4 py-16 sm:px-6">
      <p className="text-sm font-medium text-accent">Documentation</p>
      <h1 className="mt-2 text-4xl font-semibold tracking-tight">SSHVault docs</h1>
      <p className="mt-3 text-lg text-muted">Practical guides written from the project's own documentation. Start with <Link className="text-accent underline underline-offset-2" to="/docs/getting-started">Getting started</Link>.</p>
      <div className="mt-12 space-y-10">
        {SECTIONS.map((s) => (
          <section key={s} aria-labelledby={`sec-${s}`}>
            <h2 id={`sec-${s}`} className="text-sm font-semibold uppercase tracking-wider text-muted">
              {s}
            </h2>
            <ul className="mt-4 grid gap-3 sm:grid-cols-2">
              {docs.filter((d) => d.section === s).map((d) => (
                <li key={d.slug}>
                  <Link to={`/docs/${d.slug}`} className="group block h-full">
                    <Card className="h-full p-5 transition-colors group-hover:border-accent/50">
                      <h3 className="font-semibold">{d.title}</h3>
                      <p className="mt-1 text-sm text-muted">{d.description}</p>
                    </Card>
                  </Link>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}

function DocPage({ slug }: { slug: string }) {
  const doc = docBySlug(slug)!;
  usePageMeta(`${doc.title}: SSHVault docs`, doc.description);
  const [drawer, setDrawer] = useState(false);
  const Content = useMemo(() => lazy(() => loadDoc(slug)), [slug]);
  const { prev, next } = neighbours(slug);
  return (
    <div className="mx-auto grid max-w-7xl gap-10 px-4 py-10 sm:px-6 lg:grid-cols-[15rem_minmax(0,1fr)] xl:grid-cols-[15rem_minmax(0,1fr)_14rem]">
      <aside className="hidden lg:block">
        <div className="sticky top-24 max-h-[calc(100vh-7rem)] overflow-y-auto pb-8 pr-2">
          <DocsNav current={slug} />
        </div>
      </aside>
      <div className="min-w-0">
        <div className="mb-6 flex items-center gap-3 lg:hidden">
          <button type="button" onClick={() => setDrawer(true)} className="inline-flex items-center gap-2 rounded-lg border border-line px-3 py-2 text-sm text-muted hover:text-fg" aria-label="Open documentation menu">
            <Menu size={16} aria-hidden="true" /> Docs menu
          </button>
        </div>
        <Sheet open={drawer} onOpenChange={setDrawer} title="Documentation">
          <DocsNav current={slug} onNavigate={() => setDrawer(false)} />
        </Sheet>
        <nav aria-label="Breadcrumb" className="text-sm text-muted">
          <ol className="flex flex-wrap items-center gap-2">
            <li>
              <Link to="/docs" className="hover:text-fg">
                Docs
              </Link>
            </li>
            <li aria-hidden="true">/</li>
            <li>{doc.section}</li>
            <li aria-hidden="true">/</li>
            <li aria-current="page" className="text-fg">
              {doc.title}
            </li>
          </ol>
        </nav>
        <article data-pagefind-body>
          <h1 className="mt-4 text-4xl font-semibold tracking-tight" data-pagefind-meta="title">
            {doc.title}
          </h1>
          <p className="mt-3 text-lg text-muted">{doc.description}</p>
          <div className="prose prose-lg prose-doc mt-8 max-w-none prose-a:underline prose-a:underline-offset-2">
            <MDXProvider components={mdxComponents}>
              <Suspense fallback={<p className="text-muted" role="status">Loading…</p>}>
                <Content />
              </Suspense>
            </MDXProvider>
          </div>
        </article>
        <div className="mt-12 flex items-center justify-between border-t border-line-soft pt-6 text-sm">
          <a href={editUrl(slug)} rel="noopener" className="inline-flex items-center gap-2 text-muted hover:text-fg">
            <Pencil size={14} aria-hidden="true" /> Edit this page on GitHub
          </a>
        </div>
        <nav aria-label="Previous and next pages" className="mt-6 grid gap-3 sm:grid-cols-2">
          {prev ? (
            <Link to={`/docs/${prev.slug}`} className="rounded-xl border border-line p-4 hover:border-accent/50">
              <span className="text-xs text-muted">Previous</span>
              <span className="mt-1 block font-medium">{prev.title}</span>
            </Link>
          ) : (
            <span />
          )}
          {next && (
            <Link to={`/docs/${next.slug}`} className="rounded-xl border border-line p-4 text-right hover:border-accent/50">
              <span className="text-xs text-muted">Next</span>
              <span className="mt-1 block font-medium">{next.title}</span>
            </Link>
          )}
        </nav>
      </div>
      <aside className="hidden xl:block">
        <div className="sticky top-24">
          <Toc slug={slug} />
        </div>
      </aside>
    </div>
  );
}

export function DocsRedirect() {
  return <Navigate to="/docs/getting-started" replace />;
}
