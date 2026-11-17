import { Check, Copy, Info, Lightbulb, TriangleAlert } from "lucide-react";
import { useRef, useState, type ComponentProps, type ReactNode } from "react";
import { Link } from "react-router-dom";
import { cn } from "@/lib/utils";

export function Callout({ type = "note", children }: { type?: "note" | "warning" | "tip"; children: ReactNode }) {
  const map = {
    note: { icon: Info, cls: "border-accent/30 bg-accent/5", label: "Note" },
    warning: { icon: TriangleAlert, cls: "border-warning/40 bg-warning/5", label: "Warning" },
    tip: { icon: Lightbulb, cls: "border-success/30 bg-success/5", label: "Tip" },
  }[type];
  return (
    <aside className={cn("not-prose my-6 flex gap-3 rounded-xl border p-4 text-[15px] leading-relaxed", map.cls)}>
      <map.icon size={18} className="mt-0.5 shrink-0" aria-hidden="true" />
      <div className="min-w-0 [&_p]:m-0 [&_p+p]:mt-2 [&_code]:text-[13px]">
        <span className="sr-only">{map.label}: </span>
        {children}
      </div>
    </aside>
  );
}

/** A code block with a copy button. The text comes from the rendered DOM, so it is exactly what is shown. */
function Pre(props: ComponentProps<"pre">) {
  const ref = useRef<HTMLPreElement>(null);
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    const text = ref.current?.innerText ?? "";
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      /* clipboard is unavailable on insecure origins; the text stays selectable */
    }
  };
  return (
    <div className="group/code relative">
      <pre ref={ref} {...props} className={cn(props.className, "hljs")} />
      <button
        type="button"
        onClick={copy}
        aria-label={copied ? "Copied" : "Copy code"}
        className="absolute right-2.5 top-2.5 inline-flex size-8 items-center justify-center rounded-md border border-line bg-panel text-muted opacity-0 transition-opacity hover:text-fg focus-visible:opacity-100 group-hover/code:opacity-100"
      >
        {copied ? <Check size={14} className="text-success" /> : <Copy size={14} />}
      </button>
      <span role="status" aria-live="polite" className="sr-only">
        {copied ? "Copied to clipboard" : ""}
      </span>
    </div>
  );
}

function A({ href = "", children, ...rest }: ComponentProps<"a">) {
  if (href.startsWith("/")) return <Link to={href}>{children}</Link>;
  const external = /^https?:/.test(href);
  return (
    <a href={href} {...(external ? { rel: "noopener" } : {})} {...rest}>
      {children}
    </a>
  );
}

export const mdxComponents = { pre: Pre, a: A, Callout };
