import { forwardRef } from "react";

/** The pointer. Positioned imperatively by the stage (transform only), so moving it never re-renders React. */
export const SimulatedCursor = forwardRef<HTMLDivElement>(function SimulatedCursor(_props, ref) {
  return (
    <div ref={ref} data-testid="sim-cursor" aria-hidden="true" className="pointer-events-none absolute left-0 top-0 z-50 opacity-0 will-change-transform">
      <svg width="22" height="26" viewBox="0 0 22 26" className="origin-top-left drop-shadow-[0_2px_3px_rgba(0,0,0,0.55)]" data-cursor-svg>
        <path d="M2 1.5v18.2l4.7-4.4 3.2 7.2 3-1.4-3.2-7.1h6.6z" fill="#fff" stroke="#111827" strokeWidth="1.5" strokeLinejoin="round" />
      </svg>
    </div>
  );
});
