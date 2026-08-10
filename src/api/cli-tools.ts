"use strict";

import { Hono } from "hono";
import { Database } from "bun:sqlite";
import { config } from "../config";
import { existsSync } from "node:fs";
import { join } from "node:path";

const DEFAULT_9ROUTER_DB = "C:\\Users\\Asus\\AppData\\Roaming\\9router\\db\\data.sqlite";

export const cliToolsRouter = new Hono();

// CLI Tools metadata
const CLI_TOOLS_META = {
  "claude": {
    name: "Claude Code",
    description: "Claude AI coding assistant CLI",
    icon: "claude.png",
    cli: "claude",
    configPaths: [
      "~/.config/claude/config.json",
      "~/.claude/config.json",
      "~/Library/Application Support/Claude/config.json",
    ],
  },
  "openclaw": {
    name: "Open Claw",
    description: "Open-source Claude CLI",
    icon: "openclaw.png",
    cli: "openclaw",
    configPaths: ["~/.config/openclaw/config.json"],
  },
  "codex": {
    name: "OpenAI Codex CLI / App",
    description: "OpenAI's official CLI and desktop app",
    icon: "codex.png",
    cli: "codex",
    configPaths: [
      "~/.config/codex/config.json",
      "~/Library/Application Support/OpenAI Codex/config.json",
    ],
  },
  "opencode": {
    name: "OpenCode",
    description: "Open-source coding assistant",
    icon: "opencode.png",
    cli: "opencode",
    configPaths: ["~/.config/opencode/config.json"],
  },
  "cowork": {
    name: "Claude Cowork",
    description: "Claude team collaboration CLI",
    icon: "claude.png",
    cli: "cowork",
    configPaths: ["~/.config/cowork/config.json"],
  },
  "hermes": {
    name: "Hermes Agent",
    description: "Multi-provider AI agent CLI",
    icon: "hermes.png",
    cli: "hermes",
    configPaths: ["~/.config/hermes/config.json"],
  },
  "droid": {
    name: "Factory Droid",
    description: "AI-powered software factory CLI",
    icon: "droid.png",
    cli: "droid",
    configPaths: ["~/.config/droid/config.json"],
  },
  "cursor": {
    name: "Cursor",
    description: "AI-first code editor CLI",
    icon: "cursor.png",
    cli: "cursor",
    configPaths: [
      "~/.config/cursor/config.json",
      "~/Library/Application Support/Cursor/config.json",
    ],
  },
  "cline": {
    name: "Cline",
    description: "AI coding assistant for CLI",
    icon: "cline.png",
    cli: "cline",
    configPaths: ["~/.config/cline/config.json"],
  },
  "kilo": {
    name: "Kilo Code",
    description: "Kilo AI coding assistant CLI",
    icon: "kilocode.png",
    cli: "kilo",
    configPaths: ["~/.config/kilo/config.json"],
  },
  "roo": {
    name: "Roo",
    description: "AI pair programmer CLI",
    icon: "roo.png",
    cli: "roo",
    configPaths: ["~/.config/roo/config.json"],
  },
  "continue": {
    name: "Continue",
    description: "VS Code extension CLI",
    icon: "continue.png",
    cli: "continue",
    configPaths: ["~/.continue/config.json"],
  },
  "amp": {
    name: "Amp CLI",
    description: "AI-powered productivity CLI",
    icon: "amp.png",
    cli: "amp",
    configPaths: ["~/.config/amp/config.json"],
  },
  "qwen": {
    name: "Qwen Code",
    description: "Qwen AI coding assistant CLI",
    icon: "qwen.png",
    cli: "qwen",
    configPaths: ["~/.config/qwen/config.json"],
  },
  "deepseek-tui": {
    name: "DeepSeek TUI",
    description: "DeepSeek AI terminal UI",
    icon: "deepseek-tui.png",
    cli: "deepseek",
    configPaths: ["~/.config/deepseek/config.json"],
  },
  "jcode": {
    name: "jcode",
    description: "Java-focused AI coding assistant",
    icon: "jcode.png",
    cli: "jcode",
    configPaths: ["~/.config/jcode/config.json"],
  },
  "grok-build": {
    name: "Grok Build",
    description: "Grok AI build system CLI",
    icon: "grok-cli.png",
    cli: "grok",
    configPaths: ["~/.config/grok/config.json"],
  },
  "devin": {
    name: "Devin CLI",
    description: "Devin AI software engineer CLI",
    icon: "devin-cli.png",
    cli: "devin",
    configPaths: ["~/.config/devin/config.json"],
  },
};

// Check if a CLI tool is installed
function isCliInstalled(cli: string): boolean {
  try {
    const result = Bun.spawnSync([cli, "--version"], { stdout: "pipe", stderr: "pipe" });
    return result.exitCode === 0;
  } catch {
    return false;
  }
}

// Check if a config file exists
function isConfigPresent(paths: string[]): boolean {
  return paths.some((p) => {
    const expanded = p.replace(/^~/, process.env.HOME || process.env.USERPROFILE || "~");
    return existsSync(expanded);
  });
}

// Get CLI tools status from 9router
cliToolsRouter.get("/status", async (c) => {
  const sqlitePath = c.req.query("sqlitePath") || DEFAULT_9ROUTER_DB;
  if (!existsSync(sqlitePath)) {
    return c.json({ error: `9router SQLite not found at ${sqlitePath}` }, 404);
  }

  try {
    const routerDb = new Database(sqlitePath, { readonly: true });
    const cliConnections = routerDb.prepare(
      `SELECT id, provider, name, isActive, data FROM providerConnections WHERE provider LIKE 'cli-%'`
    ).all() as any[];

    const tools = Object.entries(CLI_TOOLS_META).map(([id, meta]) => {
      const conn = cliConnections.find((c) => c.provider === `cli-${id}`);
      const installed = isCliInstalled(meta.cli);
      const configured = conn ? conn.isActive : isConfigPresent(meta.configPaths);
      const status = installed ? (configured ? "connected" : "not_configured") : "not_installed";

      return {
        id,
        ...meta,
        status,
        connected: status === "connected",
        installed,
        configured,
        connectionId: conn?.id,
        lastUpdated: conn ? new Date(conn.data?.updatedAt || 0).toISOString() : null,
      };
    });

    routerDb.close();
    return c.json({ success: true, tools });
  } catch (error) {
    return c.json({ error: error instanceof Error ? error.message : "Failed to fetch CLI tools status" }, 500);
  }
});

// Connect/disconnect a CLI tool
cliToolsRouter.post("/:id/connect", async (c) => {
  const id = c.req.param("id");
  const sqlitePath = c.req.query("sqlitePath") || DEFAULT_9ROUTER_DB;
  if (!existsSync(sqlitePath)) {
    return c.json({ error: `9router SQLite not found at ${sqlitePath}` }, 404);
  }

  const meta = CLI_TOOLS_META[id as keyof typeof CLI_TOOLS_META];
  if (!meta) {
    return c.json({ error: `CLI tool ${id} not found` }, 404);
  }

  try {
    const routerDb = new Database(sqlitePath);
    const existing = routerDb.prepare(
      `SELECT id FROM providerConnections WHERE provider = ?`
    ).get(`cli-${id}`) as any;

    if (existing) {
      routerDb.prepare(`UPDATE providerConnections SET isActive = 1 WHERE id = ?`).run(existing.id);
    } else {
      routerDb.prepare(
        `INSERT INTO providerConnections (provider, name, isActive, data) VALUES (?, ?, 1, ?)`
      ).run(`cli-${id}`, meta.name, JSON.stringify({ updatedAt: new Date().toISOString() }));
    }

    routerDb.close();
    return c.json({ success: true, id, status: "connected" });
  } catch (error) {
    return c.json({ error: error instanceof Error ? error.message : "Failed to connect CLI tool" }, 500);
  }
});

cliToolsRouter.post("/:id/disconnect", async (c) => {
  const id = c.req.param("id");
  const sqlitePath = c.req.query("sqlitePath") || DEFAULT_9ROUTER_DB;
  if (!existsSync(sqlitePath)) {
    return c.json({ error: `9router SQLite not found at ${sqlitePath}` }, 404);
  }

  try {
    const routerDb = new Database(sqlitePath);
    routerDb.prepare(`UPDATE providerConnections SET isActive = 0 WHERE provider = ?`).run(`cli-${id}`);
    routerDb.close();
    return c.json({ success: true, id, status: "not_configured" });
  } catch (error) {
    return c.json({ error: error instanceof Error ? error.message : "Failed to disconnect CLI tool" }, 500);
  }
});

// List all available CLI tools
cliToolsRouter.get("/list", (c) => {
  return c.json({
    success: true,
    tools: Object.entries(CLI_TOOLS_META).map(([id, meta]) => ({ id, ...meta })),
  });
});

// Health check
cliToolsRouter.get("/health", (c) => {
  return c.json({ status: "ok" });
});

export default cliToolsRouter;