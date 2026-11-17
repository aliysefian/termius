/// <reference types="vite/client" />

declare module "virtual:docs-index" {
  const docs: {
    slug: string;
    title: string;
    description: string;
    section: string;
    order: number;
    headings: { depth: number; text: string; id: string }[];
  }[];
  export default docs;
}
