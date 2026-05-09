import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// SPA を `/` で配信、API は `/api/*` を webapp (:8080) に proxy する。
// caller (api.ts) は webapp 仕様の resource path (`/me` 等) をそのまま渡し、`/api` prefix は
// api.ts 内で付与する。proxy は `/apiary` のような prefix 衝突を避けるため境界明示の regex。
export default defineConfig({
  base: "/",
  plugins: [react()],
  server: {
    open: "/",
    proxy: {
      "^/api(?:/|$)": {
        target: "http://127.0.0.1:8080",
        changeOrigin: false,
      },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: false,
  },
});
