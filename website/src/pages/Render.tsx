import { useParams } from "react-router-dom";
import { DemoStage } from "@/components/sim/DemoStage";
import { usePlayer, useTimeline } from "@/hooks/usePlayer";
import { useTutorialDef } from "@/hooks/useTutorialDef";
import { useEffect } from "react";

/**
 * Bare stage for offline capture. The exporter opens /render/<id>, then calls
 * window.__demoPlayers[<id>].seek(t) for each frame and screenshots [data-testid="sim-stage"].
 * The clock is never started, so every frame is a pure function of t and the output is deterministic.
 */
export default function Render() {
  const { id = "" } = useParams();
  const { def } = useTutorialDef(id);
  const timeline = useTimeline(def);
  const player = usePlayer(timeline);
  useEffect(() => {
    if (!player) return;
    const reg = (window.__demoPlayers ??= {});
    reg[id] = player;
    document.documentElement.dataset.ready = "true";
    return () => {
      delete reg[id];
    };
  }, [player, id]);
  if (!player) return null;
  return (
    <div className="bg-black p-0" style={{ width: 1100 }}>
      <DemoStage player={player} />
    </div>
  );
}
