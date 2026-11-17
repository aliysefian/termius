import { useEffect, useRef } from "react";
import type { Player } from "@/engine/player";
import { useAutoplay } from "@/hooks/useAutoplay";
import { usePlayerSelect } from "@/hooks/usePlayer";
import { DemoStage } from "@/components/sim/DemoStage";
import { cn } from "@/lib/utils";
import { AnimatedProductWindow } from "./AnimatedProductWindow";
import { TutorialControls } from "./TutorialControls";
import { TutorialStepDescription } from "./TutorialStepDescription";
import { TutorialTimeline } from "./TutorialTimeline";
import { Pause, Play, RotateCcw } from "lucide-react";

declare global {
  interface Window {
    /** Players on the page by tutorial id; used by the Playwright tests and the video exporter. */
    __demoPlayers?: Record<string, Player>;
  }
}

interface Props {
  player: Player;
  /** full: timeline, controls and captions. ambient: just the window and a small pause button. */
  variant?: "full" | "ambient";
  autoplay?: boolean;
  label: string;
  className?: string;
}

export function TutorialPlayer({ player, variant = "full", autoplay = true, label, className }: Props) {
  const root = useRef<HTMLDivElement>(null);
  const { reduced, onUserAction } = useAutoplay(player, root, autoplay);

  useEffect(() => {
    const id = player.timeline.def.id;
    const reg = (window.__demoPlayers ??= {});
    reg[id] = player;
    return () => {
      if (reg[id] === player) delete reg[id];
    };
  }, [player]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.target as HTMLElement).closest("input,select,button")) return;
    if (e.key === " " || e.key === "k") {
      e.preventDefault();
      onUserAction(player.playing ? "pause" : "play");
      player.toggle();
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      player.nextStep();
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      player.prevStep();
    }
  };

  return (
    <div ref={root} role="group" aria-label={`Animated demonstration: ${label}. Silent simulation with fictional data.`} tabIndex={0} onKeyDown={onKeyDown} className={cn("group/player outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-bg rounded-2xl", className)} data-testid="tutorial-player" data-tutorial-id={player.timeline.def.id}>
      <AnimatedProductWindow>
        <div className="relative">
          <DemoStage player={player} />
          {variant === "ambient" && <AmbientButton player={player} onUserAction={onUserAction} alwaysVisible={reduced} />}
          {reduced && variant === "ambient" && <span className="absolute bottom-3 left-3 rounded-full bg-black/60 px-2.5 py-1 text-[11px] text-white">Motion reduced · press play</span>}
        </div>
      </AnimatedProductWindow>
      {variant === "full" && (
        <div className="mt-4 space-y-4">
          <TutorialTimeline player={player} />
          <TutorialControls player={player} onUserAction={onUserAction} />
          <TutorialStepDescription player={player} />
        </div>
      )}
    </div>
  );
}

function AmbientButton({ player, onUserAction, alwaysVisible }: { player: Player; onUserAction: (a: "pause" | "play") => void; alwaysVisible: boolean }) {
  const playing = usePlayerSelect(player, (p) => p.status === "playing", false);
  const ended = usePlayerSelect(player, (p) => p.status === "ended", false);
  return (
    <button
      type="button"
      aria-label={playing ? "Pause demonstration" : "Play demonstration"}
      onClick={() => {
        onUserAction(playing ? "pause" : "play");
        if (ended) player.restart();
        else player.toggle();
      }}
      className={cn("absolute bottom-3 right-3 inline-flex size-9 items-center justify-center rounded-full bg-black/60 text-white backdrop-blur transition-opacity hover:bg-black/80 focus-visible:opacity-100 group-hover/player:opacity-100", alwaysVisible ? "opacity-100" : "opacity-0")}
    >
      {playing ? <Pause size={16} /> : ended ? <RotateCcw size={16} /> : <Play size={16} />}
    </button>
  );
}
