import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export function Section({ id, eyebrow, title, lead, children, className }: { id?: string; eyebrow?: string; title: ReactNode; lead?: ReactNode; children?: ReactNode; className?: string }) {
  return (
    <section id={id} className={cn("mx-auto max-w-7xl px-4 py-20 sm:px-6 sm:py-24", className)}>
      <div className="max-w-2xl">
        {eyebrow && <p className="text-sm font-medium text-accent">{eyebrow}</p>}
        <h2 className="mt-2 text-3xl font-semibold tracking-tight sm:text-4xl">{title}</h2>
        {lead && <p className="mt-4 text-lg leading-relaxed text-muted">{lead}</p>}
      </div>
      {children && <div className="mt-12">{children}</div>}
    </section>
  );
}
