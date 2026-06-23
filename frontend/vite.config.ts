import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Stellar SDK + freighter-api both ship browser-friendly bundles but a
// couple of transitive deps still reference `global` rather than
// `globalThis`. Defining the alias keeps the bundle happy without
// dragging in node-polyfill plugins.
export default defineConfig({
  plugins: [react()],
  define: {
    global: "globalThis",
  },
  server: {
    port: 5173,
  },
});
