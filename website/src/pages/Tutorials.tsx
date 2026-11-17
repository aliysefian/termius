import { Clock, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Select } from "@/components/ui/select";
import { usePageMeta } from "@/hooks/usePageMeta";
import { categories, tutorials } from "@/tutorials/registry";
import type { Difficulty, FeatureCategory } from "@/tutorials/types";

export const fmtDuration = (s: number) => (s < 60 ? `${s} sec` : `${Math.floor(s / 60)} min ${s % 60 ? `${s % 60} sec` : ""}`.trim());

export default function Tutorials() {
  usePageMeta("Tutorials: SSHVault", "Animated, step-by-step SSHVault walkthroughs: connect, manage hosts, terminals, tmux, SFTP, tunnels, keys and running snippets on many hosts.");
  const [q, setQ] = useState("");
  const [cat, setCat] = useState<FeatureCategory | "all">("all");
  const [diff, setDiff] = useState<Difficulty | "all">("all");

  const shown = useMemo(() => {
    const s = q.trim().toLowerCase();
    return tutorials.filter((t) => (cat === "all" || t.category === cat) && (diff === "all" || t.difficulty === diff) && (!s || `${t.title} ${t.description} ${t.category} ${t.keywords.join(" ")}`.toLowerCase().includes(s)));
  }, [q, cat, diff]);

  const used = categories.filter((c) => tutorials.some((t) => t.category === c));

  return (
    <div className="mx-auto max-w-7xl px-4 py-16 sm:px-6 sm:py-20">
      <header className="max-w-3xl">
        <p className="text-sm font-medium text-accent">Tutorials</p>
        <h1 className="mt-2 text-4xl font-semibold tracking-tight sm:text-5xl">Learn by watching it happen</h1>
        <p className="mt-4 text-lg leading-relaxed text-muted">Each tutorial is a silent, self-running simulation with written steps beside it. Pause, scrub, change the speed, or step through one action at a time.</p>
      </header>

      <form role="search" aria-label="Filter tutorials" className="mt-10 flex flex-col gap-3 sm:flex-row sm:items-center" onSubmit={(e) => e.preventDefault()}>
        <label className="relative block flex-1">
          <span className="sr-only">Search tutorials</span>
          <Search size={16} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted" aria-hidden="true" />
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search tutorials, e.g. upload, tunnel, tmux" className="h-10 w-full rounded-lg border border-line bg-panel pl-9 pr-3 text-sm outline-none placeholder:text-muted focus-visible:ring-2 focus-visible:ring-accent" />
        </label>
        <label className="flex items-center gap-2 text-sm text-muted">
          Feature
          <Select value={cat} onChange={(e) => setCat(e.target.value as FeatureCategory | "all")}>
            <option value="all">All</option>
            {used.map((c) => (
              <option key={c}>{c}</option>
            ))}
          </Select>
        </label>
        <label className="flex items-center gap-2 text-sm text-muted">
          Difficulty
          <Select value={diff} onChange={(e) => setDiff(e.target.value as Difficulty | "all")}>
            <option value="all">All</option>
            <option>Beginner</option>
            <option>Intermediate</option>
          </Select>
        </label>
      </form>
      <p className="mt-4 text-sm text-muted" role="status" aria-live="polite">
        {shown.length} of {tutorials.length} tutorials
      </p>

      <ul className="mt-6 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {shown.map((t) => (
          <li key={t.id}>
            <Link to={`/tutorials/${t.id}`} className="group block h-full">
              <Card className="flex h-full flex-col p-6 transition-colors group-hover:border-accent/50 group-hover:bg-panel-hover">
                <div className="flex flex-wrap items-center gap-2">
                  <Badge variant="accent">{t.category}</Badge>
                  <Badge variant={t.difficulty === "Beginner" ? "success" : "warning"}>{t.difficulty}</Badge>
                </div>
                <h2 className="mt-4 text-lg font-semibold">{t.title}</h2>
                <p className="mt-2 flex-1 text-sm leading-relaxed text-muted">{t.description}</p>
                <p className="mt-5 flex items-center gap-4 text-xs text-muted">
                  <span className="inline-flex items-center gap-1.5">
                    <Clock size={13} aria-hidden="true" /> {fmtDuration(t.durationSec)}
                  </span>
                  <span>{t.steps.length} steps</span>
                </p>
              </Card>
            </Link>
          </li>
        ))}
      </ul>
      {shown.length === 0 && <p className="mt-10 text-center text-muted">No tutorials match. Clear a filter to see them all.</p>}
    </div>
  );
}
