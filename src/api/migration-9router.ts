import { Hono } from "hono";
import { db } from "../db/index";
import { accounts } from "../db/schema";
import { eq, and } from "drizzle-orm";
import { encrypt } from "../utils/crypto";
import { Database } from "bun:sqlite";
import { broadcast } from "../ws/index";
import { existsSync, copyFileSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { config } from "../config";

export const migration9Router = new Hono();

const DEFAULT_DB = "C:\\Users\\Asus\\AppData\\Roaming\\9router\\db\\data.sqlite";

function toAstraProvider(provider: string) {
  // 9router stores Blackbox as a native provider, but Luminus routes it
  // through the generic OpenAI-compatible BYOK adapter.
  if (provider === "blackbox" || provider.startsWith("openai-compatible")) return "byok";
  if (provider === "xai") return "xai";
  return provider;
}

function safePrefix(input: string) {
  return input.toLowerCase().replace(/[^a-z0-9-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 32) || "imported";
}

function parseData(raw: unknown) {
  try { return JSON.parse(String(raw || "{}")); } catch { return {}; }
}

// Provider-specific token mappers
function mapQoderTokens(data: any, conn: any) {
  const ps = data.providerSpecificData || {};
  return {
    source: "9router",
    original_provider: conn.provider,
    original_id: conn.id,
    personalToken: data.accessToken, // 9router stores as accessToken, Qoder needs personalToken
    refreshToken: data.refreshToken,
    userId: data.userId || ps.userId,
    userName: data.displayName || conn.name,
    email: conn.email,
    expireTime: data.expiresAt ? new Date(data.expiresAt).getTime() : undefined,
    machineId: data.machineId || ps.machineId,
    machineToken: data.machineToken || ps.machineToken,
    machineType: data.machineType || ps.machineType,
  };
}

function mapKiroTokens(data: any, conn: any) {
  const ps = data.providerSpecificData || {};
  return {
    source: "9router",
    original_provider: conn.provider,
    original_id: conn.id,
    access_token: data.accessToken,
    refresh_token: data.refreshToken,
    profile_arn: ps.profileArn || data.profileArn || ps.profile_arn,
    expires_at: data.expiresAt,
    region: ps.region || "us-east-1",
  };
}

function mapByokTokens(data: any, conn: any) {
  const ps = data.providerSpecificData || {};
  const prefix = safePrefix(ps.prefix || conn.name || conn.provider);

  // Get models from 9router's providerModels array
  const models = Array.isArray(data.providerModels) && data.providerModels.length > 0
    ? data.providerModels.map((m: any) => m.model || m).filter(Boolean)
    : ["*"]; // fallback only if no models defined

  return {
    source: "9router",
    original_provider: conn.provider,
    original_id: conn.id,
    base_url: ps.baseUrl || data.baseUrl || "",
    api_key: data.apiKey,
    format: ps.apiType === "chat" ? "openai" : "auto",
    models,
    model_prefix: prefix,
    headers: data.headers || {},
    key_label: conn.name || "9router-key",
    priority: conn.priority || 0,
  };
}

// Blackbox.ai — 9router stores it as a native provider with only apiKey (no
// baseUrl, no prefix, no model list). We map it to a BYOK group with a fixed
// prefix "blackbox" so all keys load-balance together under blackbox-* models.
// Base URL: https://api.blackbox.ai/v1 (OpenAI-compatible, per Blackbox docs).
const BLACKBOX_BASE_URL = "https://api.blackbox.ai/v1";
const BLACKBOX_KNOWN_MODELS = [
  "claude-sonnet-4",
  "claude-sonnet-4-5",
  "claude-opus-4",
  "claude-opus-4-1",
  "claude-opus-4-5",
  "gpt-4o",
  "gpt-4o-mini",
  "gpt-4.1",
  "gpt-5",
  "o1",
  "o1-mini",
  "o3",
  "o3-mini",
  "deepseek-v3",
  "deepseek-r1",
  "gemini-2.0-flash",
  "gemini-2.5-pro",
  "llama-3.3-70b",
  "Qwen2.5-72B",
];

function mapBlackboxTokens(data: any, conn: any) {
  return {
    source: "9router",
    original_provider: conn.provider,
    original_id: conn.id,
    base_url: BLACKBOX_BASE_URL,
    api_key: data.apiKey,
    format: "openai",
    // 9router doesn't expose a blackbox model list; use a broad known set so
    // the BYOK group advertises models. Routing still accepts any blackbox-* id.
    models: BLACKBOX_KNOWN_MODELS,
    model_prefix: "blackbox",
    headers: {},
    key_label: conn.name || "blackbox-key",
    priority: conn.priority || 0,
  };
}

function mapGenericTokens(data: any, conn: any) {
  const tokens: any = {
    source: "9router",
    original_provider: conn.provider,
    original_id: conn.id,
  };

  if (data.accessToken) tokens.access_token = data.accessToken;
  if (data.refreshToken) tokens.refresh_token = data.refreshToken;
  if (data.apiKey) tokens.api_key = data.apiKey;
  if (data.bearer_token) tokens.bearer_token = data.bearer_token;
  if (data.cookies) tokens.cookies = data.cookies;
  if (data.web_cookie) tokens.web_cookie = data.web_cookie;
  if (data.csrf_token) tokens.csrf_token = data.csrf_token;

  return tokens;
}

// Backup Astra DB before migration
function backupAstraDb() {
  const dbPath = config.databasePath;
  const backupDir = join(dirname(dbPath), "backups");
  mkdirSync(backupDir, { recursive: true });
  const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
  const backupPath = join(backupDir, `astra-before-9router-migration-${timestamp}.db`);
  copyFileSync(dbPath, backupPath);
  return backupPath;
}

migration9Router.post("/import-9router", async (c) => {
  const body = await c.req.json<{
    sqlitePath?: string;
    providers?: string[];
    importOpenAiCompatible?: boolean;
    importNativeProviders?: boolean;
    repairExisting?: boolean;
  }>();

  const sqlitePath = body.sqlitePath || DEFAULT_DB;
  const selected = new Set((body.providers || []).filter(Boolean));
  const importOpenAiCompatible = body.importOpenAiCompatible !== false;
  const importNativeProviders = body.importNativeProviders !== false;

  if (!existsSync(sqlitePath)) {
    return c.json({ error: `9router SQLite not found at ${sqlitePath}` }, 404);
  }

  try {
    // Backup Astra DB before import
    const backupPath = backupAstraDb();
    console.log(`[Migration] Astra DB backed up to ${backupPath}`);

    const routerDb = new Database(sqlitePath, { readonly: true });
    const connections = routerDb.prepare(`
      SELECT id, provider, name, email, priority, isActive, data
      FROM providerConnections
      WHERE isActive = 1
    `).all() as any[];

    const results = {
      imported: 0,
      skipped: 0,
      errors: 0,
      repaired: 0,
      providers: {} as Record<string, number>,
      details: [] as string[],
    };

    for (const conn of connections) {
      const originalProvider = String(conn.provider || "");
      if (selected.size && !selected.has(originalProvider)) { results.skipped++; continue; }
      if (originalProvider.startsWith("openai-compatible") && !importOpenAiCompatible) { results.skipped++; continue; }
      if (!originalProvider.startsWith("openai-compatible") && !importNativeProviders) { results.skipped++; continue; }

      try {
        const data = parseData(conn.data);
        const astraProvider = toAstraProvider(originalProvider);

        // Map tokens based on provider type
        let tokens: any;
        if (originalProvider === "blackbox") {
          tokens = mapBlackboxTokens(data, conn);
        } else if (originalProvider === "qoder") {
          tokens = mapQoderTokens(data, conn);
        } else if (originalProvider === "kiro" || originalProvider === "kiro-pro") {
          tokens = mapKiroTokens(data, conn);
        } else if (astraProvider === "byok") {
          tokens = mapByokTokens(data, conn);
        } else {
          tokens = mapGenericTokens(data, conn);
        }

        const email = originalProvider === "blackbox"
          ? `blackbox#${String(conn.id).slice(0, 12)}`
          : (conn.email || `${astraProvider}-${String(conn.id).slice(0, 8)}@9router-import`);

        // Check if exists - if repairExisting is true, update instead of skip
        const existing = await db.select().from(accounts)
          .where(and(eq(accounts.email, email), eq(accounts.provider, astraProvider)))
          .limit(1);

        if (existing.length > 0) {
          if (body.repairExisting) {
            // Update existing account with correct tokens
            await db.update(accounts)
              .set({
                tokens,
                status: "active",
                errorMessage: null,
                updatedAt: new Date(),
              })
              .where(and(eq(accounts.email, email), eq(accounts.provider, astraProvider)));
            results.repaired++;
            results.details.push(`REPAIRED: ${astraProvider}/${email}`);
          } else {
            results.skipped++;
            results.details.push(`SKIP: ${astraProvider}/${email}`);
          }
          continue;
        }

        const secret = data.apiKey || data.accessToken || data.refreshToken || "9router-import";
        await db.insert(accounts).values({
          provider: astraProvider,
          email,
          password: encrypt(secret),
          status: "active",
          enabled: true,
          tokens,
          quotaLimit: -1,
          quotaRemaining: -1,
          metadata: {
            name: conn.name,
            source: "9router",
            imported_at: new Date().toISOString(),
            original_priority: conn.priority,
          },
        });

        results.imported++;
        results.providers[astraProvider] = (results.providers[astraProvider] || 0) + 1;
        results.details.push(`OK: ${astraProvider}/${email}`);
      } catch (err) {
        results.errors++;
        results.details.push(`ERROR: ${conn.id} - ${err instanceof Error ? err.message : String(err)}`);
      }
    }

    routerDb.close();

    if (results.providers.byok) {
      try {
        const { pool } = await import("../proxy/pool");
        pool.invalidate("byok");
        const { refreshByokModels } = await import("../proxy/providers/registry");
        await refreshByokModels();
      } catch (err) {
        console.error("Failed to refresh BYOK runtime:", err);
      }
    }

    broadcast({ type: "migration_completed", data: results });
    return c.json({ success: true, summary: results, backup: backupPath });
  } catch (error) {
    return c.json({ error: error instanceof Error ? error.message : "Migration failed" }, 500);
  }
});

// Repair existing broken imports (all providers support)
migration9Router.post("/repair-9router-imports", async (c) => {
  const body = await c.req.json<{ sqlitePath?: string; providers?: string[] }>();
  const sqlitePath = body.sqlitePath || DEFAULT_DB;
  const selectedProviders = new Set((body.providers || []).filter(Boolean));

  if (!existsSync(sqlitePath)) {
    return c.json({ error: `9router SQLite not found at ${sqlitePath}` }, 404);
  }

  try {
    const backupPath = backupAstraDb();
    console.log(`[Repair] Astra DB backed up to ${backupPath}`);

    const routerDb = new Database(sqlitePath, { readonly: true });
    const allAccounts = await db.select().from(accounts);

    let repaired = 0;
    const details: string[] = [];
    const repairedProviders = new Set<string>();

    for (const account of allAccounts) {
      const tokens = typeof account.tokens === "string"
        ? JSON.parse(account.tokens)
        : (account.tokens || {});

      if (tokens.source !== "9router") continue;
      if (selectedProviders.size > 0 && !selectedProviders.has(account.provider)) {
        // Special case for blackbox: if user selects "blackbox" (9router name), match it to "byok" (Luminus name)
        // only if this account actually came from blackbox (email starts with blackbox#).
        const isBlackboxFilter = selectedProviders.has("blackbox") && account.provider === "byok" && String(account.email).startsWith("blackbox#");
        if (!isBlackboxFilter) continue;
      }

      const originalId = tokens.original_id;
      if (!originalId) {
        details.push(`SKIP: ${account.provider}/${account.email} (no original_id)`);
        continue;
      }

      // Fetch from 9router (only active connections — repair should not pull dead keys)
      const conn = routerDb.prepare(`
        SELECT id, provider, name, email, priority, isActive, data
        FROM providerConnections
        WHERE id = ? AND isActive = 1
      `).get(originalId) as any;

      if (!conn) {
        details.push(`SKIP: ${account.provider}/${account.email} (not found in 9router)`);
        continue;
      }

      const data = parseData(conn.data);
      let fixedTokens: any;

      // Map tokens based on provider type
      if (conn.provider === "blackbox") {
        fixedTokens = mapBlackboxTokens(data, conn);
      } else if (account.provider === "qoder") {
        fixedTokens = mapQoderTokens(data, conn);
      } else if (account.provider === "kiro" || account.provider === "kiro-pro") {
        fixedTokens = mapKiroTokens(data, conn);
      } else if (account.provider === "byok") {
        fixedTokens = mapByokTokens(data, conn);
      } else if (account.provider === "xai") {
        fixedTokens = mapGenericTokens(data, conn);
        fixedTokens.access_token = data.accessToken;
        fixedTokens.refresh_token = data.refreshToken;
        fixedTokens.expires_at = data.expiresAt;
        fixedTokens.api_key = data.apiKey;
      } else {
        fixedTokens = mapGenericTokens(data, conn);
      }

      await db.update(accounts)
        .set({
          tokens: fixedTokens,
          status: "active",
          errorMessage: null,
          updatedAt: new Date(),
        })
        .where(eq(accounts.id, account.id));

      repaired++;
      repairedProviders.add(account.provider);
      details.push(`REPAIRED: ${account.provider}/${account.email}`);
    }

    routerDb.close();

    // Refresh runtime caches
    if (repairedProviders.has("byok")) {
      try {
        const { pool } = await import("../proxy/pool");
        pool.invalidate("byok");
        const { refreshByokModels } = await import("../proxy/providers/registry");
        await refreshByokModels();
      } catch (err) {
        console.error("Failed to refresh BYOK runtime:", err);
      }
    }

    broadcast({ type: "accounts_updated", data: { providers: Array.from(repairedProviders) } });
    return c.json({ success: true, repaired, details, backup: backupPath });
  } catch (error) {
    return c.json({ error: error instanceof Error ? error.message : "Repair failed" }, 500);
  }
});

migration9Router.get("/preview-9router", async (c) => {
  const sqlitePath = c.req.query("path") || DEFAULT_DB;
  if (!existsSync(sqlitePath)) return c.json({ error: `9router SQLite not found` }, 404);

  try {
    const routerDb = new Database(sqlitePath, { readonly: true });
    const connections = routerDb.prepare(`
      SELECT id, provider, name, email, priority, isActive
      FROM providerConnections
    `).all() as any[];

    const summary = {
      total: connections.length,
      active: connections.filter((c) => c.isActive).length,
      byProvider: {} as Record<string, number>,
    };
    for (const conn of connections) summary.byProvider[conn.provider] = (summary.byProvider[conn.provider] || 0) + 1;
    routerDb.close();
    return c.json({ summary, connections });
  } catch (error) {
    return c.json({ error: error instanceof Error ? error.message : "Preview failed" }, 500);
  }
});
