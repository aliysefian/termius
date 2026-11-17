import { Pause, Play, RotateCcw, SkipBack, SkipForward } from "lucide-react";
import type { Player } from "@/engine/player";
import { usePlayerSelect } from "@/hooks/usePlayer";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";

export const SPEEDS = [0.5, 0.75, 1, 1.5, 2];

export function TutorialControls({ player, onUserAction }: { player: Player; onUserAction?: (action: "pause" | "play") => void }) {
  const playing = usePlayerSelect(player, (p) => p.status === "playing", false);
  const ended = usePlayerSelect(player, (p) => p.status === "ended", false);
  const speed = usePlayerSelect(player, (p) => p.speed, 1);
  return (
    <div className="flex flex-wrap items-center gap-2" role="toolbar" aria-label="Tutorial playback">
      <Button
        variant="default"
        size="sm"
        aria-label={playing ? "Pause" : ended ? "Replay" : "Play"}
        onClick={() => {
          if (playing) {
            player.pause();
            onUserAction?.("pause");
          } else {
            if (ended) player.restart();
            else player.play();
            onUserAction?.("play");
          }
        }}
      >
        {playing ? <Pause /> : ended ? <RotateCcw /> : <Play />}
        {playing ? "Pause" : ended ? "Replay" : "Play"}
      </Button>
      <Button variant="secondary" size="icon" className="size-8" aria-label="Previous step" onClick={() => player.prevStep()}>
        <SkipBack />
      </Button>
      <Button variant="secondary" size="icon" className="size-8" aria-label="Next step" onClick={() => player.nextStep()}>
        <SkipForward />
      </Button>
      <Button
        variant="ghost"
        size="sm"
        aria-label="Restart from the beginning"
        onClick={() => {
          player.restart();
          onUserAction?.("play");
        }}
      >
        <RotateCcw /> Restart
      </Button>
      <label className="ml-auto flex items-center gap-2 text-[13px] text-muted">
        Speed
        <Select value={String(speed)} onChange={(e) => player.setSpeed(Number(e.target.value))} aria-label="Playback speed">
          {SPEEDS.map((s) => (
            <option key={s} value={s}>
              {s}×
            </option>
          ))}
        </Select>
      </label>
    </div>
  );
}
