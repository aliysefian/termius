/** An expanding ring at the pointer. Remounted (via `key`) for every click; its parent is positioned by the stage. */
export function ClickIndicator({ double }: { double?: boolean }) {
  return (
    <>
      <span className="absolute block h-10 w-10 rounded-full border-2 border-sim-accent bg-sim-accent/25" style={{ animation: "sim-ripple 520ms ease-out forwards" }} />
      {double && <span className="absolute block h-10 w-10 rounded-full border-2 border-sim-accent" style={{ animation: "sim-ripple 520ms ease-out 200ms forwards", opacity: 0 }} />}
    </>
  );
}
