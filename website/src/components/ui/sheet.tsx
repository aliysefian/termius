import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/** A slide-in drawer on Radix Dialog: focus trap, Escape to close and focus return come for free. */
export function Sheet({ open, onOpenChange, title, side = "left", children }: { open: boolean; onOpenChange: (o: boolean) => void; title: string; side?: "left" | "right"; children: ReactNode }) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
        <Dialog.Content
          aria-describedby={undefined}
          className={cn("fixed top-0 z-50 flex h-full w-[min(86vw,22rem)] flex-col border-line bg-bg-elev p-4 shadow-2xl outline-none", side === "left" ? "left-0 border-r" : "right-0 border-l")}
        >
          <div className="mb-3 flex items-center justify-between">
            <Dialog.Title className="text-sm font-semibold">{title}</Dialog.Title>
            <Dialog.Close className="rounded-md p-1.5 text-muted hover:bg-panel-hover hover:text-fg" aria-label="Close">
              <X size={18} />
            </Dialog.Close>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
