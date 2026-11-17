import type { TutorialDefinition } from "@/engine/types";

export type Difficulty = "Beginner" | "Intermediate";

export type FeatureCategory = "SSH & Connections" | "Terminal" | "Host Management" | "File Management" | "tmux" | "Security" | "Networking" | "DevOps Tools";

export interface WrittenStep {
  title: string;
  body: string;
}

export interface TutorialMeta {
  id: string;
  title: string;
  description: string;
  difficulty: Difficulty;
  /** Rough running time shown in the library, in seconds. A test keeps it within 3 s of the real timeline. */
  durationSec: number;
  category: FeatureCategory;
  /** One per chapter of the timeline, in order. */
  steps: WrittenStep[];
  docs: { slug: string; label: string }[];
  related: string[];
  keywords: string[];
}

export type TutorialLoader = () => Promise<{ default: TutorialDefinition }>;
