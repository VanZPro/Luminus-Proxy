"use strict";

import { Hono } from "hono";
import { Database } from "bun:sqlite";
import { existsSync } from "node:fs";

const DEFAULT_9ROUTER_DB = "C:\\Users\\Asus\\AppData\\Roaming\\9router\\db\\data.sqlite";

export const oauthProvidersRouter = new Hono();

// OAuth Providers metadata
const OAUTH_PROVIDERS_META = {
  "claude": {
    name: "Claude Code",
    description: "Claude AI coding assistant",
    icon: "claude.png",
    providers: ["claude", "anthropic"],
  },
  "antigravity": {
    name: "Antigravity",
    description: "Antigravity AI platform",
    icon: "antigravity.png",
    providers: ["antigravity"],
  },
  "codex": {
    name: "OpenAI Codex",
    description: "OpenAI's official coding assistant",
    icon: "codex.png",
    providers: ["codex"],
  },
  "qoder": {
    name: "Qoder",
    description: "AI-powered code generation",
    icon: "qoder.png",
    providers: ["qoder"],
  },
  "github": {
    name: "GitHub Copilot",
    description: "AI pair programmer from GitHub",
    icon: "github.png",
    providers: ["github-copilot", "copilot"],
  },
  "cursor": {
    name: "Cursor IDE",
    description: "AI-first code editor",
    icon: "cursor.png",
    providers: ["cursor"],
  },
  "kilocode": {
    name: "Kilo Code",
    description: "Kilo AI coding assistant",
    icon: "kilocode.png",
    providers: ["kilo", "kilocode"],
  },
  "cline": {
    name: "Cline",
    description: "AI coding assistant",
    icon: "cline.png",
    providers: ["cline"],
  },
  "clinepass": {
    name: "ClinePass",
    description: "Cline authentication service",
    icon: "clinepass.png",
    providers: ["clinepass"],
  },
  "codebuddy-intl": {
    name: "CodeBuddy",
    description: "CodeBuddy international",
    icon: "codebuddy-intl.png",
    providers: ["codebuddy"],
  },
  "codebuddy-cn": {
    name: "CodeBuddy CN",
    description: "CodeBuddy China",
    icon: "codebuddy-cn.png",
    providers: ["codebuddy-china"],
  },
  "kimi": {
    name: "Kimi",
    description: "Kimi AI assistant",
    icon: "kimi.png",
    providers: ["kimi"],
  },
  "grok-cli": {
    name: "Grok CLI (Grok Build)",
    description: "Grok AI build system",
    icon: "grok-cli.png",
    providers: ["grok", "grok-cli"],
  },
  "xai": {
    name: "xAI (Grok)",
    description: "xAI's Grok platform",
    icon: "xai.png",
    providers: ["xai"],
  },
};

// Get OAuth providers status from 9router
oauthProvidersRouter.get("/status", async (c) => {
  const sqlitePath = c.req.query("sqlitePath") || DEFAULT_9ROUTER_DB;
  if (!existsSync(sqlitePath)) {
    return c.json({ error: `9router SQLite not found at ${sqlitePath}` }, 404);
  }

  try {
    const routerDb = new Database(sqlitePath, { readonly: true });
    const connections = routerDb.prepare(
      `SELECT id, provider, name, email, isActive, data FROM providerConnections`
    ).all() as any[];

    const providers = Object.entries(OAUTH_PROVIDERS_META).map(([id, meta]) => {
      const conns = connections.filter((c) => meta.providers.includes(c.provider));
      const activeConns = conns.filter((c) => c.isActive);

      return {
        id,
        ...meta,
        connections: activeConns.length,
        totalConnections: conns.length,
        connected: activeConns.length > 0,
        connectionIds: activeConns.map((c) => c.id),
        lastUpdated: activeConns[0] ? new Date(activeConns[0].data?.updatedAt || 0).toISOString() : null,
      };
    });

    routerDb.close();
    return c.json({ success: true, providers });
  } catch (error) {
    return c.json({ error: error instanceof Error ? error.message : "Failed to fetch OAuth providers status" }, 500);
  }
});

// Get detailed connections for a provider
oauthProvidersRouter.get("/:id/connections", async (c) => {
  const id = c.req.param("id");
  const sqlitePath = c.req.query("sqlitePath") || DEFAULT_9ROUTER_DB;
  if (!existsSync(sqlitePath)) {
    return c.json({ error: `9router SQLite not found at ${sqlitePath}` }, 404);
  }

  const meta = OAUTH_PROVIDERS_META[id as keyof typeof OAUTH_PROVIDERS_META];
  if (!meta) {
    return c.json({ error: `OAuth provider ${id} not found` }, 404);
  }

  try {
    const routerDb = new Database(sqlitePath, { readonly: true });
    const connections = routerDb.prepare(
      `SELECT id, provider, name, email, priority, isActive, data FROM providerConnections WHERE provider IN (${meta.providers.map(() => "?").join(",")})`
    ).all(...meta.providers) as any[];

    routerDb.close();
    return c.json({ success: true, id, connections });
  } catch (error) {
    return c.json({ error: error instanceof Error ? error.message : "Failed to fetch connections" }, 500);
  }
});

// Enable/disable a provider connection
oauthProvidersRouter.post("/:id/enable", async (c) => {
  const id = c.req.param("id");
  const body = await c.req.json<{ connectionId?: number }>();
  const sqlitePath = c.req.query("sqlitePath") || DEFAULT_9ROUTER_DB;
  if (!existsSync(sqlitePath)) {
    return c.json({ error: `9router SQLite not found at ${sqlitePath}` }, 404);
  }

  const meta = OAUTH_PROVIDERS_META[id as keyof typeof OAUTH_PROVIDERS_META];
  if (!meta) {
    return c.json({ error: `OAuth provider ${id} not found` }, 404);
  }

  try {
    const routerDb = new Database(sqlitePath);
    if (body.connectionId) {
      routerDb.prepare(`UPDATE providerConnections SET isActive = 1 WHERE id = ?`).run(body.connectionId);
    } else {
      routerDb.prepare(
        `UPDATE providerConnections SET isActive = 1 WHERE provider IN (${meta.providers.map(() => "?").join(",")})`
      ).run(...meta.providers);
    }
    routerDb.close();
    return c.json({ success: true, id, status: "connected" });
  } catch (error) {
    return c.json({ error: error instanceof Error ? error.message : "Failed to enable provider" }, 500);
  }
});

oauthProvidersRouter.post("/:id/disable", async (c) => {
  const id = c.req.param("id");
  const body = await c.req.json<{ connectionId?: number }>();
  const sqlitePath = c.req.query("sqlitePath") || DEFAULT_9ROUTER_DB;
  if (!existsSync(sqlitePath)) {
    return c.json({ error: `9router SQLite not found at ${sqlitePath}` }, 404);
  }

  const meta = OAUTH_PROVIDERS_META[id as keyof typeof OAUTH_PROVIDERS_META];
  if (!meta) {
    return c.json({ error: `OAuth provider ${id} not found` }, 404);
  }

  try {
    const routerDb = new Database(sqlitePath);
    if (body.connectionId) {
      routerDb.prepare(`UPDATE providerConnections SET isActive = 0 WHERE id = ?`).run(body.connectionId);
    } else {
      routerDb.prepare(
        `UPDATE providerConnections SET isActive = 0 WHERE provider IN (${meta.providers.map(() => "?").join(",")})`
      ).run(...meta.providers);
    }
    routerDb.close();
    return c.json({ success: true, id, status: "not_configured" });
  } catch (error) {
    return c.json({ error: error instanceof Error ? error.message : "Failed to disable provider" }, 500);
  }
});

// List all available OAuth providers
oauthProvidersRouter.get("/list", (c) => {
  return c.json({
    success: true,
    providers: Object.entries(OAUTH_PROVIDERS_META).map(([id, meta]) => ({ id, ...meta })),
  });
});

// Health check
oauthProvidersRouter.get("/health", (c) => {
  return c.json({ status: "ok" });
});

export default oauthProvidersRouter;