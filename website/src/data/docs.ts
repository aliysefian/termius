import docsIndex from "virtual:docs-index";
import type { ComponentType } from "react";

export type DocMeta = (typeof docsIndex)[number];
export const docs: DocMeta[] = docsIndex;
export const SECTIONS = ["Start", "Connect", "Work", "Secure", "Help"] as const;

export const docBySlug = (slug: string) => docs.find((d) => d.slug === slug);

const modules = import.meta.glob<{ default: ComponentType }>("../content/docs/*.mdx");
export function loadDoc(slug: string) {
  const loader = modules[`../content/docs/${slug}.mdx`];
  return loader ? loader() : Promise.reject(new Error(`No doc: ${slug}`));
}

export function neighbours(slug: string) {
  const i = docs.findIndex((d) => d.slug === slug);
  return { prev: i > 0 ? docs[i - 1] : undefined, next: i >= 0 && i < docs.length - 1 ? docs[i + 1] : undefined };
}

export const REPO = "https://github.com/aliysefian/termius";
export const editUrl = (slug: string) => `${REPO}/edit/main/website/src/content/docs/${slug}.mdx`;
