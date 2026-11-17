import { STAGE_H, STAGE_W } from "@/components/sim/SimApp";
import { AnimatedProductWindow } from "./AnimatedProductWindow";

/** Same aspect ratio as the stage, so loading a tutorial never shifts the layout. */
export function TutorialSkeleton({ error }: { error?: boolean }) {
  return (
    <AnimatedProductWindow>
      <div className="flex w-full items-center justify-center bg-sim-base text-sm text-sim-muted" style={{ aspectRatio: `${STAGE_W} / ${STAGE_H}` }}>
        {error ? "This demonstration could not be loaded." : "Loading demonstration…"}
      </div>
    </AnimatedProductWindow>
  );
}
