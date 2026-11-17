import type * as React from "react";
import { ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";

/** A styled native <select>: fully keyboard and screen-reader accessible without extra code. */
export function Select({ className, children, ...props }: React.ComponentProps<"select">) {
  return (
    <span className={cn("relative inline-flex items-center", className)}>
      <select
        {...props}
        className="h-8 cursor-pointer appearance-none rounded-lg border border-line bg-panel pl-2.5 pr-7 text-[13px] text-fg outline-none hover:bg-panel-hover focus-visible:ring-2 focus-visible:ring-accent"
      >
        {children}
      </select>
      <ChevronDown size={14} className="pointer-events-none absolute right-2 text-muted" aria-hidden="true" />
    </span>
  );
}
