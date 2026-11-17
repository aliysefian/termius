import { defineConfig, mergeConfig } from "vitest/config";
import viteConfig from "./vite.config";

// Kept apart from vite.config.ts so the build config stays typed against Vite alone.
export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      environment: "jsdom",
      include: ["tests/**/*.test.{ts,tsx}"],
      setupFiles: ["tests/setup.ts"],
      css: false,
    },
  }),
);
