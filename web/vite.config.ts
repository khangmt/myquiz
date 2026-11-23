/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { readFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import type { Plugin } from "vite";

// In production Caddy serves uploaded images at /media/. In development this
// does the same from MEDIA_DIR (the folder `myquiz api` writes to).
function devMedia(): Plugin {
  const dir = resolve(process.env.MEDIA_DIR ?? "../data/media");
  return {
    name: "myquiz-dev-media",
    configureServer(server) {
      server.middlewares.use("/media", async (req, res, next) => {
        const key = (req.url ?? "").replace(/^\//, "");
        if (!/^[0-9a-f]{32}\.webp$/.test(key)) return next();
        try {
          const body = await readFile(join(dir, key));
          res.setHeader("Content-Type", "image/webp");
          res.setHeader("X-Content-Type-Options", "nosniff");
          res.end(body);
        } catch {
          res.statusCode = 404;
          res.end();
        }
      });
    },
  };
}

// In development, Vite proxies /api and /ws to the Rust servers so the
// browser sees one origin, as it will behind Caddy in production.
export default defineConfig({
  plugins: [react(), tailwindcss(), devMedia()],
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
