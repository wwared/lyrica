import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

export default defineConfig({
  plugins: [preact()],
  // Relative asset URLs so the app works behind a reverse proxy at any path prefix.
  base: "./",
});
