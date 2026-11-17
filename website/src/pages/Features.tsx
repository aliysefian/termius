import { Fragment } from "react";
import { FeatureDemoCard } from "@/components/FeatureDemoCard";
import { Reveal } from "@/components/Reveal";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { demoFeatures, otherFeatures, type FeatureStatus } from "@/data/features";
import { usePageMeta } from "@/hooks/usePageMeta";
import { categories } from "@/tutorials/registry";

const statusVariant: Record<FeatureStatus, "success" | "accent" | "warning" | "default"> = { Implemented: "success", Partial: "accent", Experimental: "warning", Planned: "default" };

export default function Features() {
  usePageMeta("Features: SSHVault", "Everything SSHVault does today, each with a short animated demonstration, a documentation link and a tutorial.");
  const cats = categories.filter((c) => demoFeatures.some((f) => f.category === c));
  let n = 0;
  return (
    <div className="mx-auto max-w-7xl px-4 py-16 sm:px-6 sm:py-20">
      <header className="max-w-3xl">
        <p className="text-sm font-medium text-accent">Features</p>
        <h1 className="mt-2 text-4xl font-semibold tracking-tight sm:text-5xl">What SSHVault does today</h1>
        <p className="mt-4 text-lg leading-relaxed text-muted">
          Each capability below is implemented in the current release. The clips are silent simulations of the interface with fictional data. Anything that is partial, experimental or planned is labelled in the last section.
        </p>
      </header>

      <nav aria-label="Feature categories" className="mt-8 flex flex-wrap gap-2">
        {cats.map((c) => (
          <a key={c} href={`#cat-${c.replace(/\W+/g, "-").toLowerCase()}`} className="rounded-full border border-line px-3.5 py-1.5 text-sm text-muted hover:border-accent/50 hover:text-fg">
            {c}
          </a>
        ))}
        <a href="#more" className="rounded-full border border-line px-3.5 py-1.5 text-sm text-muted hover:border-accent/50 hover:text-fg">
          More capabilities
        </a>
      </nav>

      <div className="mt-16 space-y-24">
        {cats.map((c) => (
          <Fragment key={c}>
            <section aria-labelledby={`cat-${c.replace(/\W+/g, "-").toLowerCase()}`} id={`cat-${c.replace(/\W+/g, "-").toLowerCase()}`} className="scroll-mt-24">
              <h2 id={`cat-${c.replace(/\W+/g, "-").toLowerCase()}-h`} className="mb-8 border-b border-line-soft pb-3 text-sm font-semibold uppercase tracking-wider text-muted">
                {c}
              </h2>
              <div className="space-y-20">
                {demoFeatures
                  .filter((f) => f.category === c)
                  .map((f) => (
                    <FeatureDemoCard key={f.id} feature={f} flip={n++ % 2 === 1} />
                  ))}
              </div>
            </section>
          </Fragment>
        ))}
      </div>

      <section id="more" className="mt-28 scroll-mt-24" aria-labelledby="more-h">
        <h2 id="more-h" className="text-3xl font-semibold tracking-tight">
          More capabilities, and what is not finished
        </h2>
        <p className="mt-3 max-w-2xl text-muted">These have no animated demonstration yet. The status labels are deliberate: partial and experimental items say what is missing, and planned items are not available.</p>
        <div className="mt-10 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {otherFeatures.map((f, i) => (
            <Reveal key={f.name} delay={(i % 3) * 0.04}>
              <Card className="h-full p-5">
                <div className="flex items-start justify-between gap-3">
                  <f.icon size={18} className="mt-0.5 text-accent" aria-hidden="true" />
                  <Badge variant={statusVariant[f.status]}>{f.status}</Badge>
                </div>
                <h3 className="mt-3 font-semibold">{f.name}</h3>
                <p className="mt-1.5 text-sm leading-relaxed text-muted">{f.text}</p>
                {f.note && <p className="mt-2 text-[13px] text-warning">{f.note}</p>}
              </Card>
            </Reveal>
          ))}
        </div>
      </section>
    </div>
  );
}
