import { ChevronDown } from "lucide-react";
import { Link } from "react-router-dom";
import { faqGroups } from "@/data/faq";
import { usePageMeta } from "@/hooks/usePageMeta";

export default function Faq() {
  usePageMeta("FAQ and troubleshooting: SSHVault", "Answers about SSHVault's vault, sync, installers, host keys, the SSH agent and common problems.");
  return (
    <div className="mx-auto max-w-3xl px-4 py-16 sm:px-6 sm:py-20">
      <header>
        <p className="text-sm font-medium text-accent">FAQ and troubleshooting</p>
        <h1 className="mt-2 text-4xl font-semibold tracking-tight sm:text-5xl">Questions, answered</h1>
        <p className="mt-4 text-lg text-muted">
          Condensed from the project documentation. For the long form see <Link className="text-accent underline underline-offset-2" to="/docs/troubleshooting">Troubleshooting</Link>.
        </p>
      </header>
      <div className="mt-12 space-y-12">
        {faqGroups.map((g) => (
          <section key={g.title} aria-labelledby={`faq-${g.title}`}>
            <h2 id={`faq-${g.title}`} className="text-sm font-semibold uppercase tracking-wider text-muted">
              {g.title}
            </h2>
            <div className="mt-4 divide-y divide-line-soft rounded-2xl border border-line bg-panel/50">
              {g.items.map((f) => (
                <details key={f.q} className="group px-5 py-4 [&_summary::-webkit-details-marker]:hidden">
                  <summary className="flex cursor-pointer list-none items-center justify-between gap-4 rounded-md font-medium outline-none focus-visible:ring-2 focus-visible:ring-accent">
                    {f.q}
                    <ChevronDown size={16} className="shrink-0 text-muted transition-transform group-open:rotate-180" aria-hidden="true" />
                  </summary>
                  <p className="mt-3 leading-relaxed text-muted">{f.a}</p>
                  {f.doc && (
                    <p className="mt-2 text-sm">
                      <Link className="text-accent underline underline-offset-2" to={`/docs/${f.doc.slug}`}>
                        Read more: {f.doc.label}
                      </Link>
                    </p>
                  )}
                </details>
              ))}
            </div>
          </section>
        ))}
      </div>
      <p className="mt-12 text-sm text-muted">
        Still stuck? <a className="text-accent underline underline-offset-2" href="https://github.com/aliysefian/termius/issues" rel="noopener">Open an issue</a>. For security problems see{" "}
        <a className="text-accent underline underline-offset-2" href="https://github.com/aliysefian/termius/blob/main/SECURITY.md" rel="noopener">
          SECURITY.md
        </a>
        .
      </p>
    </div>
  );
}
