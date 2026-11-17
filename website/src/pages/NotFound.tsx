import { Link } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { usePageMeta } from "@/hooks/usePageMeta";

export default function NotFound() {
  usePageMeta("Page not found: SSHVault");
  return (
    <div className="mx-auto max-w-2xl px-6 py-32 text-center">
      <p className="text-sm font-medium text-accent">404</p>
      <h1 className="mt-2 text-4xl font-semibold tracking-tight">That page is not here</h1>
      <p className="mt-3 text-muted">It may have moved. Try the docs or the tutorials.</p>
      <div className="mt-8 flex justify-center gap-3">
        <Button asChild>
          <Link to="/">Home</Link>
        </Button>
        <Button variant="secondary" asChild>
          <Link to="/docs">Documentation</Link>
        </Button>
      </div>
    </div>
  );
}
