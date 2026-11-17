import { cn } from "@/lib/utils";
import { renderAnsi } from "./ansi";
import { demo } from "./primitives";

interface Props {
  id: string;
  lines: string[];
  /** What is being typed after the prompt right now. */
  input?: string;
  prompt?: string;
  active?: boolean;
  /** Show the caret (the pane has focus). */
  focused?: boolean;
  className?: string;
  /** Print the prompt and caret after the output (a connected shell waiting for input). */
  showPrompt?: boolean;
  title?: string;
  /** The screen holds this many lines; older ones scroll off the top, as in a real terminal. */
  maxLines?: number;
}

/** A terminal pane. It prints predefined lines only; nothing here can run a command. */
export function MockTerminal({ id, lines, input = "", prompt = "", focused = true, className, showPrompt = true, title, maxLines = 25 }: Props) {
  return (
    <div {...demo(id)} className={cn("relative flex min-h-0 flex-1 flex-col overflow-hidden bg-sim-term font-mono text-[12px] leading-[18px] text-sim-fg", className)}>
      {title && <div className="border-b border-sim-line bg-sim-base px-3 py-1 text-[11px] text-sim-muted">{title}</div>}
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden px-3 py-2">
        {lines.slice(-(maxLines - (showPrompt ? 1 : 0))).map((l, i) => (
          <div key={i} className="whitespace-pre">
            {renderAnsi(l) || " "}
          </div>
        ))}
        {showPrompt && (
          <div className="whitespace-pre">
            <span className="text-sim-success">{prompt}</span>
            <span>{input}</span>
            {focused && <span className="sim-caret inline-block h-[14px] w-[7px] translate-y-[2px] bg-sim-fg/90" />}
          </div>
        )}
      </div>
    </div>
  );
}
