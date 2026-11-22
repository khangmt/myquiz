/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// In development, Vite proxies /api and /ws to the Rust servers so the
// browser sees one origin, as it will behind Caddy in production.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      "/api": "http://localhost:8080",
      "/ws": { target: "ws://localhost:8081", ws: true },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
  },
});
