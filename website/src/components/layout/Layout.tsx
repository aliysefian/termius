import { useEffect, useRef } from "react";
import { Outlet, useLocation } from "react-router-dom";
import { Footer } from "./Footer";
import { Header } from "./Header";

/** On navigation: scroll to the hash target or the top, and move focus to the page so keyboard and screen-reader users start there. */
function RouteEffects({ main }: { main: React.RefObject<HTMLElement | null> }) {
  const { pathname, hash } = useLocation();
  const first = useRef(true);
  useEffect(() => {
    if (hash) {
      document.getElementById(decodeURIComponent(hash.slice(1)))?.scrollIntoView();
      return;
    }
    window.scrollTo(0, 0);
    if (first.current) {
      first.current = false;
      return;
    }
    main.current?.focus({ preventScroll: true });
  }, [pathname, hash, main]);
  return null;
}

export function Layout() {
  const main = useRef<HTMLElement>(null);
  return (
    <>
      <a href="#main" className="skip-link">
        Skip to content
      </a>
      <Header />
      <main id="main" ref={main} tabIndex={-1} className="outline-none">
        <Outlet />
      </main>
      <Footer />
      <RouteEffects main={main} />
    </>
  );
}
