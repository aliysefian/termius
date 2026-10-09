import { describe, expect, it } from "vitest";
import { closeAction, exitAction, type CloseContext } from "../closebehavior";

const base: CloseContext = { closeToTray: true, trayAvailable: true, connected: 0, confirmSessions: true };

describe("closing the window", () => {
  it("hides into the tray when there is one", () => {
    expect(closeAction(base)).toBe("hide");
  });

  it("keeps live sessions running instead of asking about them", () => {
    expect(closeAction({ ...base, connected: 3 })).toBe("hide");
  });

  it("never hides without a tray icon to bring the window back", () => {
    expect(closeAction({ ...base, trayAvailable: false })).toBe("close");
    expect(closeAction({ ...base, trayAvailable: false, connected: 2 })).toBe("confirm");
  });

  it("quits as before when the setting is off", () => {
    expect(closeAction({ ...base, closeToTray: false })).toBe("close");
    expect(closeAction({ ...base, closeToTray: false, connected: 1 })).toBe("confirm");
    expect(closeAction({ ...base, closeToTray: false, connected: 1, confirmSessions: false })).toBe("close");
  });
});

describe("Exit in the tray", () => {
  it("quits at once with nothing connected", () => {
    expect(exitAction({ connected: 0, confirmSessions: true })).toBe("exit");
  });

  it("asks first when live sessions would end", () => {
    expect(exitAction({ connected: 2, confirmSessions: true })).toBe("confirm");
  });

  it("doesn't ask when asking was turned off", () => {
    expect(exitAction({ connected: 2, confirmSessions: false })).toBe("exit");
  });
});
