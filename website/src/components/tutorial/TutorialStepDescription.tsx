import type { Player } from "@/engine/player";
import { usePlayerSelect } from "@/hooks/usePlayer";

/** The chapter and caption for the moment the playhead is at. */
export function TutorialStepDescription({ player }: { player: Player }) {
  const chapter = usePlayerSelect(player, (p) => p.timeline.chapterAt(p.time)?.title ?? "", "");
  const n = usePlayerSelect(player, (p) => {
    const c = p.timeline.chapterAt(p.time);
    return c ? p.timeline.chapters.indexOf(c) + 1 : 0;
  }, 0);
  const caption = usePlayerSelect(player, (p) => p.timeline.caption(p.time) ?? "", "");
  const total = player.timeline.chapters.length;
  return (
    <div className="min-h-[3.5rem]" data-testid="step-description">
      {chapter && (
        <p className="text-xs font-medium uppercase tracking-wider text-accent">
          Step {n} of {total} · {chapter}
        </p>
      )}
      <p className="mt-1 text-[15px] leading-relaxed text-fg">{caption}</p>
    </div>
  );
}
