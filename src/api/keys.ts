import { Hono } from "hono";
import { db } from "../db/index";
import { settings, apiKeys } from "../db/schema";
import { eq, and } from "drizzle-orm";
import { config } from "../config";

const API_KEY_SETTING = "api_key";
const API_KEY_CACHE_TTL_MS = 5_000;

let activeApiKeyCache: { key: string; expiresAt: number } | null = null;

export const keysRouter = new Hono();

function generateApiKey(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(24));
  const token = btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/g, "");
  return `sk-pool-${token}`;
}

// ── Legacy global key (env / settings fallback) ────────────────────────────

export async function getActiveApiKey(): Promise<string> {
  const now = Date.now();
  if (activeApiKeyCache && activeApiKeyCache.expiresAt > now) {
    return activeApiKeyCache.key;
  }

  const [row] = await db.select().from(settings).where(eq(settings.key, API_KEY_SETTING));
  const key = row?.value || config.apiKey;
  activeApiKeyCache = { key, expiresAt: now + API_KEY_CACHE_TTL_MS };
  return key;
}

export async function isValidApiKey(token: string): Promise<boolean> {
  if (!token) return false;
  if (token === config.apiKey) return true;

  // Check custom API keys table (public keys created in dashboard)
  const resolved = await resolveApiKey(token);
  if (resolved.row) return true;

  // Check legacy global key
  const active = await getActiveApiKey();
  return token === active;
}

async function saveApiKey(key: string) {
  const existing = await db.select().from(settings).where(eq(settings.key, API_KEY_SETTING));
  if (existing.length > 0) {
    await db.update(settings).set({ value: key, updatedAt: new Date() }).where(eq(settings.key, API_KEY_SETTING));
  } else {
    await db.insert(settings).values({ key: API_KEY_SETTING, value: key });
  }
  activeApiKeyCache = { key, expiresAt: Date.now() + API_KEY_CACHE_TTL_MS };
}

// ── Resolve a bearer token to a custom key row (or null = legacy key) ──────

export interface ResolvedApiKey {
  row: typeof apiKeys.$inferSelect | null; // null means legacy global key
}

export async function resolveApiKey(token: string): Promise<ResolvedApiKey> {
  if (!token) return { row: null };
  // Custom keys take priority — they may shadow a legacy global key.
  const [row] = await db.select().from(apiKeys).where(and(eq(apiKeys.key, token), eq(apiKeys.enabled, true)));
  if (!row) return { row: null };
  // Expiry check — expired keys are treated as invalid.
  if (row.expiresAt && row.expiresAt.getTime() < Date.now()) {
    return { row: null };
  }
  return { row };
}

/**
 * Check whether a resolved key is allowed to access the management API (`/api/*`).
 * - Legacy global key (row === null) → always allowed (admin).
 * - Custom key with isPublicOnly === false → allowed (admin-tier custom key).
 * - Custom key with isPublicOnly === true → NOT allowed (proxy-only key).
 */
export function isManagementAllowed(
  row: typeof apiKeys.$inferSelect | null,
): boolean {
  if (!row) return true; // legacy global key has full access
  return row.isPublicOnly !== true;
}

/**
 * Check whether a model is allowed for the given custom key.
 * - Empty allowlist = all models allowed.
 * - Non-empty allowlist = only listed models are allowed.
 * Returns { allowed: boolean; reason?: string }.
 */
export function isModelAllowedForCustomKey(
  row: typeof apiKeys.$inferSelect | null,
  model: string,
): { allowed: boolean; reason?: string } {
  if (!row) return { allowed: true }; // legacy key has no restrictions
  const allowed = (row.allowedModels as string[]) || [];
  if (allowed.length === 0) return { allowed: true };
  if (allowed.includes(model)) return { allowed: true };
  return {
    allowed: false,
    reason: `Model "${model}" is not allowed by API key "${row.name}". Allowed: ${allowed.join(", ")}`,
  };
}

/**
 * Check token quota for a model on a custom key.
 * Returns { allowed; used; limit }.
 */
export function checkModelTokenQuota(
  row: typeof apiKeys.$inferSelect | null,
  model: string,
): { allowed: boolean; used: number; limit: number } {
  if (!row) return { allowed: true, used: 0, limit: 0 }; // legacy key has no limits
  const limits = (row.modelTokenLimits as Record<string, number>) || {};
  const used = (row.modelTokensUsed as Record<string, number>) || {};
  const limit = limits[model] || 0;
  const usedCount = used[model] || 0;
  if (limit <= 0) return { allowed: true, used: usedCount, limit: 0 };
  return { allowed: usedCount < limit, used: usedCount, limit };
}

/**
 * Update usage counters for a custom API key after a successful request.
 * Re-reads the row to avoid lost updates, increments modelTokensUsed[model]
 * and totalTokensUsed, and sets lastUsedAt to now.
 */
export async function updateApiKeyUsage(
  row: typeof apiKeys.$inferSelect | null,
  model: string,
  totalTokens: number,
): Promise<void> {
  if (!row || totalTokens <= 0) return;
  try {
    // Re-read to get the latest counters (avoid lost updates from concurrent requests)
    const [fresh] = await db.select().from(apiKeys).where(eq(apiKeys.id, row.id));
    if (!fresh) return;
    const modelTokensUsed = (fresh.modelTokensUsed as Record<string, number>) || {};
    modelTokensUsed[model] = (modelTokensUsed[model] || 0) + totalTokens;
    const totalTokensUsed = (fresh.totalTokensUsed || 0) + totalTokens;
    await db
      .update(apiKeys)
      .set({
        modelTokensUsed,
        totalTokensUsed,
        lastUsedAt: new Date(),
        updatedAt: new Date(),
      })
      .where(eq(apiKeys.id, fresh.id));
  } catch (err) {
    console.error("[API Keys] Failed to update key usage:", err);
  }
}

// ── Legacy routes (global key) ─────────────────────────────────────────────

keysRouter.get("/", async (c) => {
  const key = await getActiveApiKey();
  return c.json({ key, source: key === config.apiKey ? "env" : "database" });
});

keysRouter.post("/regenerate", async (c) => {
  const key = generateApiKey();
  await saveApiKey(key);
  return c.json({ key, source: "database" });
});

keysRouter.post("/set", async (c) => {
  const body = await c.req.json<{ key: string }>();
  if (!body.key || body.key.length < 16) {
    return c.json({ error: "API key must be at least 16 characters" }, 400);
  }
  await saveApiKey(body.key);
  return c.json({ key: body.key, source: "database" });
});

keysRouter.post("/test", async (c) => {
  const body = await c.req.json<{ key: string }>();
  const valid = await isValidApiKey(body.key || "");
  return c.json({ valid });
});

// ── Custom API keys CRUD ───────────────────────────────────────────────────

// List all custom keys
keysRouter.get("/custom", async (c) => {
  const rows = await db.select().from(apiKeys).orderBy(apiKeys.createdAt);
  return c.json({ data: rows });
});

// Get one custom key by id
keysRouter.get("/custom/:id", async (c) => {
  const id = Number(c.req.param("id"));
  const [row] = await db.select().from(apiKeys).where(eq(apiKeys.id, id));
  if (!row) return c.json({ error: "Key not found" }, 404);
  return c.json({ data: row });
});

// Create custom key
keysRouter.post("/custom", async (c) => {
  const body = await c.req.json<{
    name: string;
    key?: string;
    allowedModels?: string[];
    modelTokenLimits?: Record<string, number>;
    totalTokenLimit?: number;
    isPublicOnly?: boolean;
    expiresAt?: string | null;
  }>();
  if (!body.name || !body.name.trim()) {
    return c.json({ error: "Name is required" }, 400);
  }
  const key = body.key || generateApiKey();
  // Guard: an expiry in the past would make a freshly created key instantly invalid.
  // Treat a past/expired expiresAt as "no expiry" so new public keys work immediately.
  let expiry: Date | null = null;
  if (body.expiresAt) {
    const parsed = new Date(body.expiresAt);
    if (Number.isNaN(parsed.getTime())) {
      expiry = null;
    } else if (parsed.getTime() <= Date.now()) {
      expiry = null;
      console.warn("[API Keys] Ignoring past expiry for new key, treated as no-expiry.");
    } else {
      expiry = parsed;
    }
  }
  const [created] = await db.insert(apiKeys).values({
    name: body.name.trim(),
    key,
    allowedModels: body.allowedModels || [],
    modelTokenLimits: body.modelTokenLimits || {},
    totalTokenLimit: body.totalTokenLimit || 0,
    isPublicOnly: body.isPublicOnly !== false, // default to public-only
    expiresAt: expiry,
  }).returning();
  return c.json({ data: created }, 201);
});

// Update custom key
keysRouter.put("/custom/:id", async (c) => {
  const id = Number(c.req.param("id"));
  const body = await c.req.json<{
    name?: string;
    allowedModels?: string[];
    modelTokenLimits?: Record<string, number>;
    totalTokenLimit?: number;
    enabled?: boolean;
    isPublicOnly?: boolean;
    expiresAt?: string | null;
  }>();
  const updates: Record<string, any> = { updatedAt: new Date() };
  if (body.name !== undefined) updates.name = body.name.trim();
  if (body.allowedModels !== undefined) updates.allowedModels = body.allowedModels;
  if (body.modelTokenLimits !== undefined) updates.modelTokenLimits = body.modelTokenLimits;
  if (body.totalTokenLimit !== undefined) updates.totalTokenLimit = body.totalTokenLimit;
  if (body.enabled !== undefined) updates.enabled = body.enabled;
  if (body.isPublicOnly !== undefined) updates.isPublicOnly = body.isPublicOnly;
  if (body.expiresAt !== undefined) {
    updates.expiresAt = body.expiresAt ? new Date(body.expiresAt) : null;
  }
  const [updated] = await db.update(apiKeys).set(updates).where(eq(apiKeys.id, id)).returning();
  if (!updated) return c.json({ error: "Key not found" }, 404);
  return c.json({ data: updated });
});

// Delete custom key
keysRouter.delete("/custom/:id", async (c) => {
  const id = Number(c.req.param("id"));
  const [deleted] = await db.delete(apiKeys).where(eq(apiKeys.id, id)).returning();
  if (!deleted) return c.json({ error: "Key not found" }, 404);
  return c.json({ success: true });
});

// Toggle enabled/disabled
keysRouter.post("/custom/:id/toggle", async (c) => {
  const id = Number(c.req.param("id"));
  const [existing] = await db.select().from(apiKeys).where(eq(apiKeys.id, id));
  if (!existing) return c.json({ error: "Key not found" }, 404);
  const [updated] = await db.update(apiKeys).set({ enabled: !existing.enabled, updatedAt: new Date() }).where(eq(apiKeys.id, id)).returning();
  return c.json({ data: updated });
});

// Test a custom key against a model
keysRouter.post("/custom/:id/test", async (c) => {
  const id = Number(c.req.param("id"));
  const body = await c.req.json<{ model?: string }>();
  const [existing] = await db.select().from(apiKeys).where(eq(apiKeys.id, id));
  if (!existing) return c.json({ error: "Key not found" }, 404);
  const model = body.model || "default";
  const allowed = isModelAllowedForCustomKey(existing, model);
  return c.json({
    valid: true,
    allowed: allowed.allowed,
    reason: allowed.reason || null,
    quota: checkModelTokenQuota(existing, model),
  });
});
