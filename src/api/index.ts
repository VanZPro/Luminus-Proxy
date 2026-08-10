import { Hono } from "hono";
import os from "os";
import { accountsRouter } from "./accounts";
import { proxySettingsRouter } from "./proxy-settings";
import { statsRouter } from "./stats";
import { keysRouter } from "./keys";

import { imageStudioRouter } from "./image-studio";
import { filtersRouter } from "./filters";
import { binApi } from "./bin";
import { integrationRouter } from "./integration";
import { oauthRouter } from "./oauth";
import { migration9Router } from "./migration-9router";
import { cliToolsRouter } from "./cli-tools";
import { oauthProvidersRouter } from "./oauth-providers";

export const apiRouter = new Hono();

apiRouter.route("/accounts", accountsRouter);
apiRouter.route("/settings", proxySettingsRouter);
apiRouter.route("/stats", statsRouter);
apiRouter.route("/keys", keysRouter);

apiRouter.route("/image-studio", imageStudioRouter);
apiRouter.route("/filters", filtersRouter);
apiRouter.route("/bin", binApi);
apiRouter.route("/integration", integrationRouter);
apiRouter.route("/oauth", oauthRouter);
apiRouter.route("/migration", migration9Router);
apiRouter.route("/cli-tools", cliToolsRouter);
apiRouter.route("/oauth-providers", oauthProvidersRouter);

apiRouter.get("/providers", (c) => {
  return c.json({ data: ["kiro", "kiro-pro", "codebuddy", "codebuddy-china", "canva", "codex", "qoder", "gitlab-duo", "youmind", "grok-cli", "byok", "xai"] });
});

// Health check — basic liveness + runtime resource snapshot.
apiRouter.get("/health", (c) => {
  const mem = process.memoryUsage?.() ?? null;
  return c.json({
    status: "ok",
    uptime: process.uptime(),
    timestamp: new Date().toISOString(),
    memory: mem
      ? {
          rss: mem.rss,
          heapUsed: mem.heapUsed,
          heapTotal: mem.heapTotal,
          external: mem.external,
          arrayBuffers: mem.arrayBuffers,
          // Bun runtime overhead ≈ RSS - JS heap - external - array buffers.
          bunRuntime: Math.max(0, mem.rss - mem.heapUsed - mem.external - mem.arrayBuffers),
        }
      : null,
    pid: typeof process.pid === "number" ? process.pid : null,
    // best-effort CPU load — process.cpuUsage may be undefined on some runtimes.
    cpu: typeof process.cpuUsage === "function" ? process.cpuUsage() : null,
    system: {
      cores: os.cpus().length,
      totalmem: os.totalmem(),
    }
  });
});
