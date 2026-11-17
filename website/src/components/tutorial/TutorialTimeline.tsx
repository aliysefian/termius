import type { Player } from "@/engine/player";
import { usePlayerSnapshot } from "@/hooks/usePlayer";

const fmt = (ms: number) => {
  const s = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

/** A scrubber with chapter ticks. A real range input, so it works with keyboard and screen readers. */
export function TutorialTimeline({ player }: { player: Player }) {
  const { time } = usePlayerSnapshot(player);
  const { timeline, range } = player;
  const [a, b] = range;
  const pct = b > a ? ((time - a) / (b - a)) * 100 : 0;
  const chapter = timeline.chapterAt(time);
  return (
    <div className="flex items-center gap-3">
      <span className="w-10 text-right font-mono text-xs tabular-nums text-muted" aria-hidden="true">
        {fmt(time)}
      </span>
      <div className="relative h-6 flex-1">
        <div className="absolute inset-x-0 top-1/2 h-1.5 -translate-y-1/2 overflow-hidden rounded-full bg-line">
          <div className="h-full rounded-full bg-accent-solid" style={{ width: `${pct}%` }} />
        </div>
        {timeline.chapters.map((c) => {
          const left = ((c.start - a) / (b - a)) * 100;
          if (left <= 0 || left >= 100) return null;
          return <span key={c.id} aria-hidden="true" className="absolute top-1/2 h-3 w-0.5 -translate-y-1/2 rounded bg-fg/40" style={{ left: `${left}%` }} />;
        })}
        <span aria-hidden="true" className="pointer-events-none absolute top-1/2 size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-bg bg-accent shadow" style={{ left: `${pct}%` }} />
        <input
          type="range"
          min={a}
          max={b}
          step={50}
          value={Math.round(time)}
          onChange={(e) => player.seek(Number(e.target.value))}
          aria-label="Seek the tutorial"
          aria-valuetext={`${fmt(time)} of ${fmt(b)}${chapter ? `, ${chapter.title}` : ""}`}
          className="absolute inset-0 h-full w-full cursor-pointer opacity-0"
        />
      </div>
      <span className="w-10 font-mono text-xs tabular-nums text-muted" aria-hidden="true">
        {fmt(b)}
      </span>
    </div>
  );
}
