import { useTimeline, usePlayer } from "@/hooks/usePlayer";
import { useTutorialDef } from "@/hooks/useTutorialDef";
import { TutorialPlayer } from "./TutorialPlayer";
import { TutorialSkeleton } from "./TutorialSkeleton";

interface Props {
  id: string;
  label: string;
  variant?: "full" | "ambient";
  /** Loop a chapter (by title) of the tutorial, e.g. for a feature clip. */
  chapter?: string;
  loop?: boolean;
  className?: string;
}

/** Loads a tutorial lazily and plays it. Used for the home page and feature clips. */
export function AutoTutorial({ id, label, variant = "ambient", chapter, loop = true, className }: Props) {
  const { def, error } = useTutorialDef(id);
  const timeline = useTimeline(def);
  const range = timeline && chapter ? timeline.range(chapter) : undefined;
  const player = usePlayer(timeline, { loop: loop ? { hold: 1800 } : undefined, range });
  if (!player) return <TutorialSkeleton error={error} />;
  return <TutorialPlayer player={player} variant={variant} label={label} className={className} />;
}
