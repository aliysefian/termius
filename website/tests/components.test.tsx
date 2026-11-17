import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it } from "vitest";
import { Player } from "@/engine/player";
import { Timeline } from "@/engine/timeline";
import { SimApp } from "@/components/sim/SimApp";
import { TutorialControls } from "@/components/tutorial/TutorialControls";
import { TutorialStepDescription } from "@/components/tutorial/TutorialStepDescription";
import { TutorialTimeline } from "@/components/tutorial/TutorialTimeline";
import { loadTutorial, tutorialIds } from "@/tutorials/registry";

const noRaf = { request: () => 1, cancel: () => {}, now: () => 0 };

async function makePlayer(id = "create-ssh-connection") {
  return new Player(new Timeline(await loadTutorial(id)), { scheduler: noRaf });
}

beforeAll(() => {
  // jsdom has no layout engine; the stage code needs these only for measuring.
  globalThis.CSS ??= { escape: (s: string) => s } as typeof CSS;
});

describe("SimApp", () => {
  for (const id of tutorialIds) {
    it(`renders ${id} at every chapter without throwing`, async () => {
      const tl = new Timeline(await loadTutorial(id));
      for (const t of [0, ...tl.chapters.map((c) => c.start + 1), tl.duration]) {
        const { unmount } = render(<SimApp state={tl.stateAt(t)} />);
        unmount();
      }
    });
  }

  it("shows typed text from state", async () => {
    const tl = new Timeline(await loadTutorial("create-ssh-connection"));
    const item = tl.items.find((i) => i.step.type === "type" && i.step.target === "f-hostname")!;
    const { container } = render(<SimApp state={tl.stateAt(item.end)} />);
    expect(container.textContent).toContain("203.0.113.50");
  });
});

describe("TutorialControls", () => {
  it("toggles play and pause and changes speed", async () => {
    const player = await makePlayer();
    render(<TutorialControls player={player} />);
    fireEvent.click(screen.getByRole("button", { name: "Play" }));
    expect(player.status).toBe("playing");
    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    expect(player.status).toBe("paused");
    fireEvent.change(screen.getByLabelText("Playback speed"), { target: { value: "2" } });
    expect(player.speed).toBe(2);
  });

  it("steps forward and back", async () => {
    const player = await makePlayer();
    render(<TutorialControls player={player} />);
    fireEvent.click(screen.getByRole("button", { name: "Next step" }));
    expect(player.time).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("button", { name: "Restart from the beginning" }));
    expect(player.time).toBe(0);
  });
});

describe("TutorialTimeline and description", () => {
  it("scrubbing seeks and the caption follows", async () => {
    const player = await makePlayer();
    render(
      <>
        <TutorialTimeline player={player} />
        <TutorialStepDescription player={player} />
      </>,
    );
    const slider = screen.getByRole("slider", { name: "Seek the tutorial" });
    await act(async () => fireEvent.change(slider, { target: { value: String(player.timeline.chapters[1]!.start + 100) } }));
    expect(player.time).toBe(player.timeline.chapters[1]!.start + 100);
    expect(screen.getByTestId("step-description").textContent).toContain("Fill in the connection");
  });
});
