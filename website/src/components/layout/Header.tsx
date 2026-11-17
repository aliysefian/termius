import { Menu, Search } from "lucide-react";
import { GithubIcon } from "@/components/icons";
import { useEffect, useRef, useState } from "react";
import { Link, NavLink } from "react-router-dom";
import { Logo } from "@/components/Logo";
import { Button } from "@/components/ui/button";
import { Sheet } from "@/components/ui/sheet";
import { REPO_URL } from "@/data/releases";
import { cn } from "@/lib/utils";
import { SearchDialog } from "./SearchDialog";
import { ThemeToggle } from "./ThemeToggle";

export const NAV = [
  { to: "/features", label: "Features" },
  { to: "/tutorials", label: "Tutorials" },
  { to: "/docs", label: "Docs" },
  { to: "/downloads", label: "Downloads" },
  { to: "/faq", label: "FAQ" },
];

const linkClass = ({ isActive }: { isActive: boolean }) => cn("rounded-md px-3 py-1.5 text-sm transition-colors hover:text-fg", isActive ? "text-fg" : "text-muted");

export function Header() {
  const [search, setSearch] = useState(false);
  const [menu, setMenu] = useState(false);
  // The dialog has no Radix trigger, so remember what opened it and hand focus back on close.
  const opener = useRef<HTMLElement | null>(null);
  const openSearch = () => {
    opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setSearch(true);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        if (search) setSearch(false);
        else openSearch();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [search]);

  return (
    <header className="sticky top-0 z-40 border-b border-line-soft bg-bg/80 backdrop-blur-xl">
      <div className="mx-auto flex h-16 max-w-7xl items-center gap-2 px-4 sm:px-6">
        <Link to="/" className="mr-4 flex items-center gap-2.5 rounded-md font-semibold tracking-tight" aria-label="SSHVault home">
          <Logo size={28} />
          <span className="text-[17px]">SSHVault</span>
        </Link>
        <nav aria-label="Main" className="hidden items-center md:flex">
          {NAV.map((n) => (
            <NavLink key={n.to} to={n.to} className={linkClass}>
              {n.label}
            </NavLink>
          ))}
        </nav>
        <div className="ml-auto flex items-center gap-1.5">
          <button
            type="button"
            onClick={openSearch}
            className="hidden h-9 w-56 items-center gap-2 rounded-lg border border-line bg-panel/60 px-3 text-sm text-muted transition-colors hover:bg-panel-hover lg:flex"
          >
            <Search size={15} aria-hidden="true" />
            <span className="flex-1 text-left">Search</span>
            <kbd aria-hidden="true" className="rounded border border-line px-1.5 text-[11px]">
              Ctrl K
            </kbd>
          </button>
          <Button variant="ghost" size="icon" className="lg:hidden" aria-label="Search" onClick={openSearch}>
            <Search />
          </Button>
          <Button variant="ghost" size="icon" asChild>
            <a href={REPO_URL} aria-label="SSHVault on GitHub" rel="noopener">
              <GithubIcon />
            </a>
          </Button>
          <ThemeToggle />
          <Button size="sm" className="ml-1 hidden sm:inline-flex" asChild>
            <Link to="/downloads">Download</Link>
          </Button>
          <Button variant="ghost" size="icon" className="md:hidden" aria-label="Open menu" onClick={() => setMenu(true)}>
            <Menu />
          </Button>
        </div>
      </div>
      <Sheet open={menu} onOpenChange={setMenu} title="Menu" side="right">
        <nav aria-label="Mobile" className="flex flex-col gap-1">
          {NAV.map((n) => (
            <NavLink key={n.to} to={n.to} onClick={() => setMenu(false)} className={({ isActive }) => cn("rounded-lg px-3 py-3 text-base", isActive ? "bg-panel-hover text-fg" : "text-muted")}>
              {n.label}
            </NavLink>
          ))}
          <Button className="mt-3" asChild>
            <Link to="/downloads" onClick={() => setMenu(false)}>
              Download
            </Link>
          </Button>
        </nav>
      </Sheet>
      <SearchDialog open={search} onOpenChange={setSearch} returnFocus={opener} />
    </header>
  );
}
