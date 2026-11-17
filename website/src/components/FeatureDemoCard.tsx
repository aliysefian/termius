import { useRef, useState, useEffect } from "react";
import { ArrowRight, BookOpen, PlayCircle } from "lucide-react";
import { Link } from "react-router-dom";
import { AutoTutorial } from "@/components/tutorial/AutoTutorial";
import { TutorialSkeleton } from "@/components/tutorial/TutorialSkeleton";
import { Badge } from "@/components/ui/badge";
import type { DemoFeature } from "@/data/features";
import { cn } from "@/lib/utils";

/** A feature with its looping clip. The clip mounts when the card nears the viewport and plays only while visible. */
export function FeatureDemoCard({ feature, flip }: { feature: DemoFeature; flip?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const [near, setNear] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!el || near) return;
    if (typeof IntersectionObserver === "undefined") return setNear(true);
    const io = new IntersectionObserver(([e]) => e?.isIntersecting && setNear(true), { rootMargin: "300px" });
    io.observe(el);
    return () => io.disconnect();
  }, [near]);

  return (
    <article id={feature.id} className="scroll-mt-24 grid items-center gap-8 lg:grid-cols-2 lg:gap-14" aria-labelledby={`${feature.id}-title`}>
      <div className={cn(flip && "lg:order-2")}>
        <div className="flex items-center gap-3">
          <span className="flex size-10 items-center justify-center rounded-xl bg-accent-solid/15 text-accent">
            <feature.icon size={20} aria-hidden="true" />
          </span>
          <Badge variant="success">{feature.status}</Badge>
        </div>
        <h3 id={`${feature.id}-title`} className="mt-4 text-2xl font-semibold tracking-tight">
          {feature.name}
        </h3>
        <p className="mt-3 leading-relaxed text-muted">{feature.summary}</p>
        <p className="mt-4 rounded-lg border border-line-soft bg-panel/60 px-4 py-3 text-sm">
          <span className="font-medium text-fg">Use case: </span>
          <span className="text-muted">{feature.useCase}</span>
        </p>
        <div className="mt-5 flex flex-wrap gap-x-6 gap-y-2 text-sm font-medium">
          <Link to={`/docs/${feature.docs.slug}`} className="inline-flex items-center gap-1.5 text-accent hover:underline">
            <BookOpen size={15} aria-hidden="true" /> Docs: {feature.docs.label}
          </Link>
          <Link to={`/tutorials/${feature.tutorial}`} className="inline-flex items-center gap-1.5 text-accent hover:underline">
            <PlayCircle size={15} aria-hidden="true" /> Tutorial <ArrowRight size={14} aria-hidden="true" />
          </Link>
        </div>
      </div>
      <div ref={ref} className={cn(flip && "lg:order-1")}>
        {near ? <AutoTutorial id={feature.clip.tutorial} chapter={feature.clip.chapter} label={`${feature.name} (clip)`} /> : <TutorialSkeleton />}
      </div>
    </article>
  );
}
