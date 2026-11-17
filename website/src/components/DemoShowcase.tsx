import * as Tabs from "@radix-ui/react-tabs";
import { ArrowRight } from "lucide-react";
import { Link } from "react-router-dom";
import { AutoTutorial } from "@/components/tutorial/AutoTutorial";
import { getTutorial } from "@/tutorials/registry";

const TABS = [
  { id: "create-ssh-connection", label: "Connect", chapter: undefined },
  { id: "sftp-file-transfer", label: "Transfer files", chapter: undefined },
  { id: "tmux-sessions", label: "tmux", chapter: undefined },
  { id: "port-forwarding", label: "Tunnels", chapter: undefined },
  { id: "run-on-many-hosts", label: "Many hosts", chapter: undefined },
];

/** One live demo at a time: switching tabs unmounts the previous player, so nothing animates off screen. */
export function DemoShowcase() {
  return (
    <Tabs.Root defaultValue={TABS[0]!.id}>
      <Tabs.List aria-label="Choose a demonstration" className="mb-6 flex flex-wrap gap-2">
        {TABS.map((t) => (
          <Tabs.Trigger key={t.id} value={t.id} className="rounded-full border border-line px-4 py-2 text-sm text-muted transition-colors hover:text-fg data-[state=active]:border-accent-solid data-[state=active]:bg-accent-solid/15 data-[state=active]:text-fg">
            {t.label}
          </Tabs.Trigger>
        ))}
      </Tabs.List>
      {TABS.map((t) => {
        const meta = getTutorial(t.id)!;
        return (
          <Tabs.Content key={t.id} value={t.id} className="outline-none">
            <div className="grid items-start gap-8 lg:grid-cols-[1fr_20rem]">
              <AutoTutorial id={t.id} label={meta.title} />
              <div>
                <h3 className="text-xl font-semibold">{meta.title}</h3>
                <p className="mt-2 text-muted">{meta.description}</p>
                <ol className="mt-5 space-y-2.5 text-sm text-muted">
                  {meta.steps.map((s, i) => (
                    <li key={s.title} className="flex gap-3">
                      <span className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full bg-accent-solid/20 text-[11px] font-semibold text-accent">{i + 1}</span>
                      {s.title}
                    </li>
                  ))}
                </ol>
                <Link to={`/tutorials/${t.id}`} className="mt-6 inline-flex items-center gap-1.5 text-sm font-medium text-accent hover:underline">
                  Follow the full tutorial <ArrowRight size={14} />
                </Link>
              </div>
            </div>
          </Tabs.Content>
        );
      })}
    </Tabs.Root>
  );
}
