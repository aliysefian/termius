import { describe, expect, it } from "vitest";
import { dialogs } from "../dialogs.svelte";

describe("in-app dialogs", () => {
  it("delete questions are dangerous and labelled with the action", async () => {
    const answer = dialogs.ask('Delete snippet "backup"?');
    expect(dialogs.pending).toMatchObject({ danger: true, confirm: "Delete", title: "Are you sure?" });
    dialogs.close(true);
    expect(await answer).toBe(true);
    expect(dialogs.pending).toBeNull();

    const other = dialogs.ask("Open 12 tabs?");
    expect(dialogs.pending).toMatchObject({ danger: false, confirm: "Continue" });
    dialogs.close(false);
    expect(await other).toBe(false);
  });

  it("queues questions instead of losing one", async () => {
    const a = dialogs.ask("Remove the recovery key?");
    const b = dialogs.askText("New folder name", "logs");
    expect(dialogs.pending?.kind).toBe("confirm");
    dialogs.close(false);
    expect(dialogs.pending).toMatchObject({ kind: "text", value: "logs" });
    dialogs.close("logs-2");
    expect(await a).toBe(false);
    expect(await b).toBe("logs-2");
  });

  it("a cancelled text prompt is null", async () => {
    const t = dialogs.askText("Rename", "a.txt");
    dialogs.close(null);
    expect(await t).toBeNull();
  });
});
