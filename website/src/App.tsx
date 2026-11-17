import { lazy, Suspense } from "react";
import { Route, Routes } from "react-router-dom";
import { Layout } from "@/components/layout/Layout";

const Home = lazy(() => import("@/pages/Home"));
const Features = lazy(() => import("@/pages/Features"));
const Tutorials = lazy(() => import("@/pages/Tutorials"));
const TutorialPage = lazy(() => import("@/pages/TutorialPage"));
const Docs = lazy(() => import("@/pages/Docs"));
const Downloads = lazy(() => import("@/pages/Downloads"));
const Faq = lazy(() => import("@/pages/Faq"));
const NotFound = lazy(() => import("@/pages/NotFound"));
const Render = lazy(() => import("@/pages/Render"));

function Fallback() {
  return <div className="mx-auto min-h-[60vh] max-w-7xl px-6 py-24 text-muted" role="status">Loading…</div>;
}

export function App() {
  return (
    <Suspense fallback={<Fallback />}>
      <Routes>
        {/* Bare route used by the offline video exporter and the tests; no site chrome. */}
        <Route path="/render/:id" element={<Render />} />
        <Route element={<Layout />}>
          <Route index element={<Home />} />
          <Route path="features" element={<Features />} />
          <Route path="tutorials" element={<Tutorials />} />
          <Route path="tutorials/:id" element={<TutorialPage />} />
          <Route path="docs" element={<Docs />} />
          <Route path="docs/:slug" element={<Docs />} />
          <Route path="downloads" element={<Downloads />} />
          <Route path="faq" element={<Faq />} />
          <Route path="*" element={<NotFound />} />
        </Route>
      </Routes>
    </Suspense>
  );
}
