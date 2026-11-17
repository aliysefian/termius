import { Link } from "react-router-dom";
import { Logo } from "@/components/Logo";
import { REPO_URL, RELEASES_URL } from "@/data/releases";

const cols = [
  {
    title: "Product",
    links: [
      { to: "/features", label: "Features" },
      { to: "/tutorials", label: "Tutorials" },
      { to: "/downloads", label: "Downloads" },
    ],
  },
  {
    title: "Learn",
    links: [
      { to: "/docs/getting-started", label: "Getting started" },
      { to: "/docs", label: "Documentation" },
      { to: "/faq", label: "FAQ" },
      { to: "/docs/troubleshooting", label: "Troubleshooting" },
    ],
  },
];

export function Footer() {
  return (
    <footer className="mt-24 border-t border-line-soft">
      <div className="mx-auto grid max-w-7xl gap-10 px-4 py-14 sm:px-6 md:grid-cols-[1.5fr_1fr_1fr_1fr]">
        <div>
          <div className="flex items-center gap-2.5 font-semibold">
            <Logo size={26} /> SSHVault
          </div>
          <p className="mt-3 max-w-xs text-sm leading-relaxed text-muted">An SSH client for people who live in terminals, with an encrypted vault you sync yourself.</p>
        </div>
        {cols.map((c) => (
          <nav key={c.title} aria-label={c.title}>
            <h2 className="text-sm font-semibold">{c.title}</h2>
            <ul className="mt-3 space-y-2 text-sm text-muted">
              {c.links.map((l) => (
                <li key={l.to}>
                  <Link className="hover:text-fg" to={l.to}>
                    {l.label}
                  </Link>
                </li>
              ))}
            </ul>
          </nav>
        ))}
        <nav aria-label="Project">
          <h2 className="text-sm font-semibold">Project</h2>
          <ul className="mt-3 space-y-2 text-sm text-muted">
            {[
              [REPO_URL, "GitHub"],
              [RELEASES_URL, "Releases"],
              [`${REPO_URL}/blob/main/CHANGELOG.md`, "Changelog"],
              [`${REPO_URL}/blob/main/ROADMAP.md`, "Roadmap"],
              [`${REPO_URL}/blob/main/SECURITY.md`, "Report a vulnerability"],
              [`${REPO_URL}/blob/main/LICENSE`, "MIT license"],
            ].map(([href, label]) => (
              <li key={href}>
                <a className="hover:text-fg" href={href} rel="noopener">
                  {label}
                </a>
              </li>
            ))}
          </ul>
        </nav>
      </div>
      <div className="border-t border-line-soft py-5 text-center text-xs text-muted">
        MIT licensed · Animated demonstrations are simulations with fictional data, not recordings of a real server.
      </div>
    </footer>
  );
}
