import { Apple, Download, Monitor, Terminal as TerminalIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { LATEST_URL, platforms, RELEASES_URL, VERIFIED_VERSION } from "@/data/releases";
import { usePageMeta } from "@/hooks/usePageMeta";
import { Link } from "react-router-dom";

const icons = { windows: Monitor, macos: Apple, linux: TerminalIcon };

export default function Downloads() {
  usePageMeta("Download SSHVault", "SSHVault installers for Windows, macOS and Linux (.deb, .rpm, AppImage), straight from GitHub Releases.");
  return (
    <div className="mx-auto max-w-5xl px-4 py-16 sm:px-6 sm:py-20">
      <header>
        <p className="text-sm font-medium text-accent">Downloads</p>
        <h1 className="mt-2 text-4xl font-semibold tracking-tight sm:text-5xl">Download SSHVault</h1>
        <p className="mt-4 max-w-2xl text-lg text-muted">Every installer is a file on the project's GitHub Releases page; nothing is hosted here. The newest release is always at the link below.</p>
        <div className="mt-6 flex flex-wrap items-center gap-3">
          <Button size="lg" asChild>
            <a href={LATEST_URL} rel="noopener">
              <Download /> Latest release on GitHub
            </a>
          </Button>
          <Button size="lg" variant="secondary" asChild>
            <a href={RELEASES_URL} rel="noopener">
              All releases
            </a>
          </Button>
        </div>
        <p className="mt-4 text-sm text-muted">
          The direct links below point at <strong className="text-fg">v{VERIFIED_VERSION}</strong>, the release whose assets were checked when this page was last updated. A newer release may exist.
        </p>
      </header>

      <div className="mt-12 space-y-6">
        {platforms.map((p) => {
          const Icon = icons[p.id];
          return (
            <Card key={p.id} className="p-6 sm:p-8">
              <section aria-labelledby={`dl-${p.id}`}>
                <div className="flex flex-wrap items-center gap-3">
                  <span className="flex size-10 items-center justify-center rounded-xl bg-accent-solid/15 text-accent">
                    <Icon size={20} aria-hidden="true" />
                  </span>
                  <h2 id={`dl-${p.id}`} className="text-2xl font-semibold">
                    {p.name}
                  </h2>
                  <Badge>{p.requirement}</Badge>
                </div>
                <ul className="mt-6 divide-y divide-line-soft">
                  {p.installers.map((i) => (
                    <li key={i.file} className="flex flex-col gap-3 py-4 sm:flex-row sm:items-center sm:justify-between">
                      <div className="min-w-0">
                        <p className="font-medium">{i.label}</p>
                        <p className="mt-0.5 break-all font-mono text-xs text-muted">{i.file}</p>
                        <p className="mt-1.5 break-all font-mono text-[13px] text-fg/80">{i.how}</p>
                      </div>
                      <Button variant="secondary" size="sm" asChild className="shrink-0">
                        <a href={i.url} rel="noopener" aria-label={`Download ${i.file}`}>
                          <Download /> Download
                        </a>
                      </Button>
                    </li>
                  ))}
                </ul>
                <ul className="mt-4 list-disc space-y-1.5 pl-5 text-sm text-muted">
                  {p.notes.map((n) => (
                    <li key={n}>{n}</li>
                  ))}
                </ul>
              </section>
            </Card>
          );
        })}
      </div>

      <section className="mt-12 rounded-2xl border border-line bg-panel/50 p-6 text-sm leading-relaxed text-muted">
        <h2 className="text-base font-semibold text-fg">Verifying a download</h2>
        <p className="mt-2">
          Signing and checksums are built into the release scripts but switch on only once the signing keys are added, so current releases are not signed. See <Link className="text-accent underline underline-offset-2" to="/docs/install">Install</Link> for what to check today and the project's{" "}
          <a className="text-accent underline underline-offset-2" href="https://github.com/aliysefian/termius/blob/main/docs/SIGNING.md" rel="noopener">
            signing notes
          </a>
          .
        </p>
      </section>
    </div>
  );
}
