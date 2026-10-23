import { defineConfig } from "vite";
export default defineConfig({
  server: {
    watch: { ignored: ["**/target/**", "**/Strata/**", "**/vLLM-apple/**"] },
    proxy: {
      "/studio": "http://127.0.0.1:8080",
      "/v1": "http://127.0.0.1:8080",
      "/status": "http://127.0.0.1:8080",
    },
  },
});
