import { GithubIcon } from "@/components/icons";
import { ArrowRight, BookOpen, Download, KeyRound, Lock, PlayCircle, ShieldCheck, Terminal, Workflow } from "lucide-react";
import { Link } from "react-router-dom";
import { DemoShowcase } from "@/components/DemoShowcase";
import { Reveal } from "@/components/Reveal";
import { Section } from "@/components/Section";
import { AutoTutorial } from "@/components/tutorial/AutoTutorial";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { demoFeatures } from "@/data/features";
import { LATEST_URL, REPO_URL, VERIFIED_VERSION } from "@/data/releases";
import { usePageMeta } from "@/hooks/usePageMeta";

const HIGHLIGHTS = [
  { icon: Lock, title: "A vault you sync yourself", text: "Hosts, keys and passwords are encrypted on your computer and synced through a folder you already use. No account, no server.", to: "/docs/vault-and-sync" },
  { icon: Terminal, title: "Terminals that stay connected", text: "Tabs, up to six split panes, search, session recording and themes. Tabs keep running in the background.", to: "/features#terminal" },
  { icon: Workflow, title: "Built for real fleets", text: "Groups with shared credentials and bastions, production guard rails, and one command on many hosts at once.", to: "/features#snippets" },
  { icon: KeyRound, title: "Serious about keys", text: "A Key Manager and a built-in SSH agent so git and ssh use vault keys, with nothing written to disk.", to: "/features#keys" },
];

export default function Home() {
  usePageMeta("SSHVault: an SSH client with an encrypted vault you sync yourself", "Open-source SSH client for Linux, Windows and macOS. Hosts, keys and credentials are encrypted on your computer and synced through a folder you already use.");
  return (
    <>
      {/* Hero */}
      <div className="relative isolate overflow-hidden">
        <div className="bg-grid absolute inset-x-0 top-0 -z-10 h-[40rem] opacity-60" aria-hidden="true" />
        <div className="absolute left-1/2 top-0 -z-10 h-[34rem] w-[60rem] -translate-x-1/2 rounded-full bg-accent-solid/20 blur-[120px]" aria-hidden="true" />
        <div className="mx-auto max-w-7xl px-4 pb-8 pt-16 text-center sm:px-6 sm:pt-24">
          <Reveal>
            <a href={`${REPO_URL}/releases`} className="inline-flex" rel="noopener">
              <Badge variant="accent" className="gap-2 py-1 pl-1 pr-3 hover:bg-accent/20">
                <span className="rounded-full bg-accent-solid px-2 py-0.5 text-[11px] font-semibold text-on-accent">v{VERIFIED_VERSION}</span>
                Open source · MIT · Linux, Windows, macOS
              </Badge>
            </a>
          </Reveal>
          <Reveal delay={0.05}>
            <h1 className="mx-auto mt-6 max-w-4xl text-balance text-4xl font-semibold tracking-tight sm:text-6xl sm:leading-[1.05]">
              An SSH client for people who live in <span className="text-gradient">terminals</span>
            </h1>
          </Reveal>
          <Reveal delay={0.1}>
            <p className="mx-auto mt-6 max-w-2xl text-balance text-lg leading-relaxed text-muted">
              Hosts, keys and credentials are encrypted on your computer and synced through a folder you already use: Dropbox, Nextcloud, Syncthing. There is no account and no server, and your sync service only ever stores ciphertext.
            </p>
          </Reveal>
          <Reveal delay={0.15}>
            <div className="mt-9 flex flex-wrap items-center justify-center gap-3">
              <Button size="lg" asChild>
                <Link to="/downloads">
                  <Download /> Download SSHVault
                </Link>
              </Button>
              <Button size="lg" variant="secondary" asChild>
                <a href={REPO_URL} rel="noopener">
                  <GithubIcon /> View on GitHub
                </a>
              </Button>
              <Button size="lg" variant="ghost" asChild>
                <Link to="/tutorials">
                  <PlayCircle /> Watch the tutorials
                </Link>
              </Button>
            </div>
          </Reveal>
          <Reveal delay={0.2} className="mx-auto mt-14 max-w-5xl text-left">
            <AutoTutorial id="hero" label="Connecting to a host and running a command" />
            <p className="mt-3 text-center text-xs text-muted">A silent, self-running simulation with fictional data. Hover it to pause.</p>
          </Reveal>
        </div>
      </div>

      {/* Highlights */}
      <Section eyebrow="Why SSHVault" title="Your data, your sync, your terminals" lead="Everything the README promises, with a short demonstration of each capability that is implemented today.">
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          {HIGHLIGHTS.map((h, i) => (
            <Reveal key={h.title} delay={i * 0.05}>
              <Link to={h.to} className="group block h-full">
                <Card className="h-full p-6 transition-colors group-hover:border-accent/50 group-hover:bg-panel-hover">
                  <span className="flex size-10 items-center justify-center rounded-xl bg-accent-solid/15 text-accent">
                    <h.icon size={20} />
                  </span>
                  <h3 className="mt-5 font-semibold">{h.title}</h3>
                  <p className="mt-2 text-sm leading-relaxed text-muted">{h.text}</p>
                </Card>
              </Link>
            </Reveal>
          ))}
        </div>
      </Section>

      {/* Demos */}
      <Section eyebrow="See it work" title="Interactive demonstrations" lead="These are generated in your browser, not recorded: a simulated cursor drives a replica of the interface. Pick one, scrub it, change the speed.">
        <DemoShowcase />
      </Section>

      {/* Capability grid */}
      <Section eyebrow="Capabilities" title="Everything in one window">
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          {demoFeatures.map((f, i) => (
            <Reveal key={f.id} delay={(i % 4) * 0.05}>
              <Link to={`/features#${f.id}`} className="group block h-full">
                <Card className="flex h-full flex-col p-5 transition-colors group-hover:border-accent/50">
                  <f.icon size={20} className="text-accent" aria-hidden="true" />
                  <h3 className="mt-4 font-semibold">{f.name}</h3>
                  <p className="mt-1.5 line-clamp-3 text-sm text-muted">{f.useCase}</p>
                  <span className="mt-auto inline-flex items-center gap-1 pt-4 text-sm text-accent">
                    See it <ArrowRight size={14} className="transition-transform group-hover:translate-x-0.5" />
                  </span>
                </Card>
              </Link>
            </Reveal>
          ))}
        </div>
      </Section>

      {/* Security */}
      <Section eyebrow="Security and privacy" title="The sync service never sees your hosts" lead="What follows is how the vault works today, taken from the project's security design.">
        <div className="grid items-start gap-10 lg:grid-cols-2">
          <ul className="space-y-5">
            {[
              ["A random 256-bit vault key", "Unlocked by your master password through Argon2id, or by the recovery key."],
              ["Every record encrypted on its own", "XChaCha20-Poly1305, bound to its vault and ID, so tampered or swapped files are rejected."],
              ["Secrets stay in the Rust backend", "The interface sees redacted copies, and the vault key is wiped when it locks."],
              ["Explicit host-key trust", "Fingerprints are pinned and shared across devices; a changed key shows old and new."],
            ].map(([t, d]) => (
              <li key={t} className="flex gap-4">
                <ShieldCheck className="mt-0.5 shrink-0 text-success" size={20} aria-hidden="true" />
                <div>
                  <h3 className="font-medium">{t}</h3>
                  <p className="mt-1 text-sm text-muted">{d}</p>
                </div>
              </li>
            ))}
            <li className="rounded-xl border border-warning/30 bg-warning/5 p-4 text-sm text-muted">
              <strong className="text-fg">Not protected:</strong> a computer that is already compromised while the vault is unlocked. The master password is never stored; without it or the recovery key nobody can open the vault, including you.
            </li>
          </ul>
          <VaultDiagram />
        </div>
        <div className="mt-8">
          <Button variant="secondary" asChild>
            <Link to="/docs/security">
              Read the security model <ArrowRight />
            </Link>
          </Button>
        </div>
      </Section>

      {/* Docs */}
      <Section eyebrow="Learn" title="Start with the docs">
        <div className="grid gap-4 md:grid-cols-3">
          {[
            { to: "/docs/getting-started", icon: BookOpen, t: "Getting started", d: "Create a vault, add hosts, open it on a second computer." },
            { to: "/tutorials", icon: PlayCircle, t: "Animated tutorials", d: "Eight walkthroughs with written steps, speed control and replay." },
            { to: "/docs/troubleshooting", icon: ShieldCheck, t: "Troubleshooting", d: "Host-key changes, agent errors, blank terminals and sync problems." },
          ].map((c) => (
            <Link key={c.to} to={c.to} className="group">
              <Card className="h-full p-6 transition-colors group-hover:border-accent/50">
                <c.icon className="text-accent" size={22} aria-hidden="true" />
                <h3 className="mt-4 font-semibold">{c.t}</h3>
                <p className="mt-1.5 text-sm text-muted">{c.d}</p>
              </Card>
            </Link>
          ))}
        </div>
      </Section>

      {/* Final CTA */}
      <section className="mx-auto max-w-7xl px-4 sm:px-6">
        <div className="relative overflow-hidden rounded-3xl border border-line bg-panel/70 px-6 py-16 text-center">
          <div className="absolute left-1/2 top-0 -z-0 h-64 w-[40rem] -translate-x-1/2 rounded-full bg-accent-solid/25 blur-[100px]" aria-hidden="true" />
          <div className="relative">
            <h2 className="text-3xl font-semibold tracking-tight sm:text-4xl">Try SSHVault on your next server</h2>
            <p className="mx-auto mt-3 max-w-xl text-fg">Free and open source. Windows, macOS and Linux installers on every release.</p>
            <div className="mt-8 flex flex-wrap justify-center gap-3">
              <Button size="lg" asChild>
                <Link to="/downloads">
                  <Download /> Download
                </Link>
              </Button>
              <Button size="lg" variant="secondary" asChild>
                <a href={LATEST_URL} rel="noopener">
                  Latest release on GitHub
                </a>
              </Button>
            </div>
          </div>
        </div>
      </section>
    </>
  );
}

/** A diagram of what lives where. Plain SVG, no animation needed. */
function VaultDiagram() {
  return (
    <Card className="p-6" role="img" aria-label="Diagram: plaintext exists only on your computer; the sync folder holds encrypted records.">
      <div className="grid gap-4 sm:grid-cols-[1fr_auto_1fr] sm:items-center">
        <div className="rounded-xl border border-line bg-bg-elev p-4">
          <p className="text-xs font-semibold uppercase tracking-wide text-success">Your computer</p>
          <p className="mt-2 text-sm text-muted">Hosts, keys and passwords in plain form, only while the vault is unlocked.</p>
          <div className="mt-3 rounded-md bg-code p-2 font-mono text-[11px] text-fg">prod-api · demo@192.0.2.10</div>
        </div>
        <div className="flex items-center justify-center gap-2 text-xs text-muted sm:flex-col">
          <Lock size={16} className="text-accent" />
          <span>encrypt</span>
          <ArrowRight className="sm:rotate-0" size={16} />
        </div>
        <div className="rounded-xl border border-line bg-bg-elev p-4">
          <p className="text-xs font-semibold uppercase tracking-wide text-warning">Your sync folder</p>
          <p className="mt-2 text-sm text-muted">Random file names and ciphertext. Sizes and change times are visible; contents are not.</p>
          <div className="mt-3 rounded-md bg-code p-2 font-mono text-[11px] text-muted">9f3a…c1.rec · 5c0e…7d.rec</div>
        </div>
      </div>
    </Card>
  );
}
