import { ArrowLeft, ArrowRight, BookOpen, Clock } from "lucide-react";
import { Link, useParams } from "react-router-dom";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { TutorialPlayer } from "@/components/tutorial/TutorialPlayer";
import { TutorialSkeleton } from "@/components/tutorial/TutorialSkeleton";
import { usePageMeta } from "@/hooks/usePageMeta";
import { usePlayer, usePlayerSelect, useTimeline } from "@/hooks/usePlayer";
import { useTutorialDef } from "@/hooks/useTutorialDef";
import type { Player } from "@/engine/player";
import { getTutorial, tutorials } from "@/tutorials/registry";
import { cn } from "@/lib/utils";
import { fmtDuration } from "./Tutorials";
import NotFound from "./NotFound";

export default function TutorialPage() {
  const { id = "" } = useParams();
  const meta = getTutorial(id);
  if (!meta) return <NotFound />;
  return <TutorialView key={id} id={id} />;
}

function TutorialView({ id }: { id: string }) {
  const meta = getTutorial(id)!;
  usePageMeta(`${meta.title}: SSHVault tutorial`, meta.description);
  const { def, error } = useTutorialDef(id);
  const timeline = useTimeline(def);
  const player = usePlayer(timeline);
  const i = tutorials.findIndex((t) => t.id === id);
  const prev = tutorials[i - 1];
  const next = tutorials[i + 1];
  const related = meta.related.map((r) => getTutorial(r)).filter((t) => t !== undefined);

  return (
    <div className="mx-auto max-w-7xl px-4 py-12 sm:px-6 sm:py-16">
      <nav aria-label="Breadcrumb" className="text-sm text-muted">
        <ol className="flex items-center gap-2">
          <li>
            <Link to="/tutorials" className="hover:text-fg">
              Tutorials
            </Link>
          </li>
          <li aria-hidden="true">/</li>
          <li aria-current="page" className="text-fg">
            {meta.title}
          </li>
        </ol>
      </nav>
      <header className="mt-6 max-w-3xl">
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant="accent">{meta.category}</Badge>
          <Badge variant={meta.difficulty === "Beginner" ? "success" : "warning"}>{meta.difficulty}</Badge>
          <Badge>
            <Clock size={12} aria-hidden="true" /> {fmtDuration(meta.durationSec)}
          </Badge>
        </div>
        <h1 className="mt-4 text-4xl font-semibold tracking-tight">{meta.title}</h1>
        <p className="mt-3 text-lg text-muted">{meta.description}</p>
      </header>

      <div className="mt-10 grid items-start gap-10 xl:grid-cols-[minmax(0,1fr)_22rem]">
        <div>
          {player ? <TutorialPlayer player={player} label={meta.title} /> : <TutorialSkeleton error={error} />}
          <p className="mt-4 text-xs text-muted">This is a simulation built from the app's layout with fictional data (documentation-only addresses). It never connects to a server. Keyboard: Space plays or pauses, Left and Right step.</p>
        </div>
        <aside aria-label="Written steps">
          <h2 className="text-lg font-semibold">Step by step</h2>
          {player ? <WrittenSteps player={player} steps={meta.steps} /> : <StaticSteps steps={meta.steps} />}
          <h2 className="mt-8 text-sm font-semibold uppercase tracking-wide text-muted">Read more</h2>
          <ul className="mt-3 space-y-2 text-sm">
            {meta.docs.map((d) => (
              <li key={d.slug}>
                <Link to={`/docs/${d.slug}`} className="inline-flex items-center gap-2 text-accent hover:underline">
                  <BookOpen size={14} aria-hidden="true" /> {d.label}
                </Link>
              </li>
            ))}
          </ul>
        </aside>
      </div>

      <nav aria-label="Tutorial navigation" className="mt-16 grid gap-3 sm:grid-cols-2">
        {prev ? (
          <Button variant="secondary" size="lg" className="h-auto justify-start py-3 text-left" asChild>
            <Link to={`/tutorials/${prev.id}`}>
              <ArrowLeft />
              <span>
                <span className="block text-xs font-normal text-muted">Previous</span>
                {prev.title}
              </span>
            </Link>
          </Button>
        ) : (
          <span />
        )}
        {next && (
          <Button variant="secondary" size="lg" className="h-auto justify-end py-3 text-right" asChild>
            <Link to={`/tutorials/${next.id}`}>
              <span>
                <span className="block text-xs font-normal text-muted">Next</span>
                {next.title}
              </span>
              <ArrowRight />
            </Link>
          </Button>
        )}
      </nav>

      {related.length > 0 && (
        <section className="mt-14" aria-labelledby="related-h">
          <h2 id="related-h" className="text-xl font-semibold">
            Related tutorials
          </h2>
          <ul className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {related.map((r) => (
              <li key={r.id}>
                <Link to={`/tutorials/${r.id}`} className="group block h-full">
                  <Card className="h-full p-5 transition-colors group-hover:border-accent/50">
                    <Badge variant="accent">{r.category}</Badge>
                    <h3 className="mt-3 font-semibold">{r.title}</h3>
                    <p className="mt-1.5 text-sm text-muted">{r.description}</p>
                  </Card>
                </Link>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

function StaticSteps({ steps }: { steps: { title: string; body: string }[] }) {
  return (
    <ol className="mt-4 space-y-4">
      {steps.map((s, i) => (
        <li key={s.title} className="flex gap-3">
          <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-accent-solid/20 text-xs font-semibold text-accent">{i + 1}</span>
          <div>
            <h3 className="text-sm font-medium">{s.title}</h3>
            <p className="mt-1 text-sm text-muted">{s.body}</p>
          </div>
        </li>
      ))}
    </ol>
  );
}

/** The written steps, with the one the animation is on marked and each one able to jump the animation to it. */
function WrittenSteps({ player, steps }: { player: Player; steps: { title: string; body: string }[] }) {
  const current = usePlayerSelect(player, (p) => {
    const c = p.timeline.chapterAt(p.time);
    return c ? p.timeline.chapters.indexOf(c) : -1;
  }, -1);
  return (
    <ol className="mt-4 space-y-2">
      {steps.map((s, i) => (
        <li key={s.title}>
          <button
            type="button"
            onClick={() => {
              player.seek(player.timeline.chapterStart(i));
              player.play();
            }}
            aria-current={i === current ? "step" : undefined}
            className={cn("flex w-full gap-3 rounded-xl border px-3.5 py-3 text-left transition-colors", i === current ? "border-accent/50 bg-accent-solid/10" : "border-transparent hover:bg-panel-hover")}
          >
            <span className={cn("flex size-6 shrink-0 items-center justify-center rounded-full text-xs font-semibold", i === current ? "bg-accent-solid text-on-accent" : "bg-accent-solid/20 text-accent")}>{i + 1}</span>
            <span>
              <span className="block text-sm font-medium">{s.title}</span>
              <span className="mt-1 block text-sm leading-relaxed text-muted">{s.body}</span>
            </span>
          </button>
        </li>
      ))}
    </ol>
  );
}
