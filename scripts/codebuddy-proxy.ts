#!/usr/bin/env bun
/**
 * Local CodeBuddy OpenAI-compatible proxy for 9router.
 *
 * Listens on 127.0.0.1:20130
 * - Injects system message if missing
 * - Always streams to upstream CodeBuddy
 * - If client requested stream=false, buffers SSE and returns JSON
 * - Round-robins active codebuddy tokens from Luminus DB
 *
 * Usage:
 *   bun scripts/codebuddy-proxy.ts
 *   CODEBUDDY_PROXY_PORT=20130 bun scripts/codebuddy-proxy.ts
 */

import { Database } from "bun:sqlite";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { existsSync } from "node:fs";

const root = fileURLToPath(new URL("..", import.meta.url));
const PORT = Number(process.env.CODEBUDDY_PROXY_PORT || 20130);
const HOST = process.env.CODEBUDDY_PROXY_HOST || "127.0.0.1";
const UPSTREAM = "https://www.codebuddy.ai/v2/chat/completions";
const PROXY_KEY =
  process.env.CODEBUDDY_PROXY_KEY ||
  "sk-codebuddy-proxy-local";

// cb/<upstream-id> -> upstream model names (same map as Luminus provider)
const CB_MODEL_MAP: Record<string, string> = {
  "cb/kimi-k3": "kimi-k3",
  "cb/kimi-k3(max)": "kimi-k3(max)",
  "cb/claude-opus-4.6": "claude-opus-4.6",
  "cb/claude-opus-4.7-1m": "claude-opus-4.7-1m",
  "cb/gemini-3.1-pro": "gemini-3.1-pro",
  "cb/gpt-5.3-codex": "gpt-5.3-codex",
  "cb/gpt-5.4": "gpt-5.4",
  "cb/gpt-5.5": "gpt-5.5",
  "cb/gpt-5.6-luna": "gpt-5.6-luna",
  "cb/gpt-5.6-sol": "gpt-5.6-sol",
  "cb/gpt-5.6-terra": "gpt-5.6-terra",
  "cb/glm-5.2": "glm-5.2",
  "cb/glm-5.1": "glm-5.1",
  "cb/glm-5.0": "glm-5.0",
  "cb/glm-5v-turbo": "glm-5v-turbo",
  "cb/minimax-m3": "minimax-m3",
  "cb/kimi-k2.6": "kimi-k2.6",
  "cb/kimi-k2.5": "kimi-k2.5",
};

const MODELS = Object.keys(CB_MODEL_MAP).map((id) => ({
  id,
  object: "model",
  created: Math.floor(Date.now() / 1000),
  owned_by: "codebuddy",
}));

type AccountTok = {
  id: number;
  email: string;
  access_token: string;
  refresh_token?: string | null;
  fail: number;
  cooldownUntil: number;
};

function findDbPath(): string {
  const candidates = [
    process.env.DATABASE_PATH,
    join(root, "data", "poolprox3.db"),
    join(root, "data", "pool.db"),
  ].filter(Boolean) as string[];
  for (const p of candidates) {
    if (existsSync(p)) return p;
  }
  throw new Error(`Luminus DB not found. Tried: ${candidates.join(", ")}`);
}

function loadAccounts(): AccountTok[] {
  const dbPath = findDbPath();
  const db = new Database(dbPath, { readonly: true });
  const rows = db
    .prepare(
      `SELECT id, email, tokens, status, enabled
       FROM accounts
       WHERE provider = 'codebuddy' AND enabled = 1 AND status = 'active'`
    )
    .all() as Array<{ id: number; email: string; tokens: string }>;

  const out: AccountTok[] = [];
  for (const r of rows) {
    try {
      const t = typeof r.tokens === "string" ? JSON.parse(r.tokens) : r.tokens;
      const access = t?.access_token || t?.bearer_token || t?.api_key;
      if (!access) continue;
      out.push({
        id: r.id,
        email: r.email,
        access_token: access,
        refresh_token: t?.refresh_token || null,
        fail: 0,
        cooldownUntil: 0,
      });
    } catch {}
  }
  db.close();
  return out;
}

let accounts = loadAccounts();
let rr = 0;
const cooldownMs = 30_000;

function pickAccount(): AccountTok | null {
  if (!accounts.length) return null;
  const now = Date.now();
  for (let i = 0; i < accounts.length; i++) {
    const idx = (rr + i) % accounts.length;
    const a = accounts[idx]!;
    if (a.cooldownUntil > now) continue;
    rr = (idx + 1) % accounts.length;
    return a;
  }
  return null;
}

function markFail(a: AccountTok) {
  a.fail++;
  a.cooldownUntil = Date.now() + cooldownMs;
}

function markOk(a: AccountTok) {
  a.fail = 0;
  a.cooldownUntil = 0;
}

function resolveModel(model: string): string {
  let m = (model || "").trim();
  // strip 9router prefixes like cb/cb-haiku-4.5 or ai/cb-haiku-4.5
  if (m.includes("/")) m = m.split("/").pop() || m;
  let lower = m.toLowerCase();

  // underscore UI forms: claude_haiku_4_5 -> claude-haiku-4.5
  if (lower.includes("_")) {
    lower = lower
      .replace(/_/g, "-")
      .replace(/-(?=\d+$)/, ".") // trailing -4-5 style last segment often version
      .replace(/(\d+)-(\d+)$/, "$1.$2");
    // common fixes
    lower = lower
      .replace("claude-haiku-4-5", "claude-haiku-4.5")
      .replace("claude-sonnet-4-6", "claude-sonnet-4.6")
      .replace("claude-opus-4-8", "claude-opus-4.8")
      .replace("claude-opus-4-7", "claude-opus-4.7")
      .replace("claude-opus-4-6", "claude-opus-4.6");
    m = lower;
  }

  if (CB_MODEL_MAP[lower]) return CB_MODEL_MAP[lower]!;
  // already upstream name
  if (
    lower.startsWith("claude-") ||
    lower.startsWith("gpt-") ||
    lower.startsWith("gemini-") ||
    lower.startsWith("kimi-")
  ) {
    return m;
  }
  // bare haiku/sonnet/opus -> map to cb defaults (never kp/ym/cbc)
  if (lower.startsWith("kp-") || lower.startsWith("ym-") || lower.startsWith("cbc-")) {
    return m; // will fail upstream clearly
  }
  if (lower.includes("haiku")) return "claude-haiku-4.5";
  if (lower.includes("sonnet")) return "claude-sonnet-4.6";
  if (lower.includes("opus")) return "claude-opus-4.8";
  return m;
}

function authOk(req: Request): boolean {
  const h = req.headers.get("authorization") || "";
  if (!h) return true; // allow open local proxy
  const token = h.replace(/^Bearer\s+/i, "").trim();
  // accept proxy key OR any non-empty key (9router may send Luminus key)
  return token.length > 0;
}

function injectSystem(messages: any[]): any[] {
  const msgs = Array.isArray(messages) ? [...messages] : [];
  const hasSystem = msgs.some((m) => m && m.role === "system");
  if (!hasSystem) {
    msgs.unshift({ role: "system", content: "You are a helpful AI assistant." });
  }
  return msgs;
}

async function aggregateSse(res: Response, model: string) {
  const reader = res.body?.getReader();
  if (!reader) throw new Error("no body");
  const decoder = new TextDecoder();
  let buffer = "";
  let content = "";
  let id = `chatcmpl-${crypto.randomUUID().slice(0, 8)}`;
  let finish: string | null = "stop";
  let usage = { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 };
  const toolCalls: any[] = [];

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() || "";
    for (const line of lines) {
      const t = line.trim();
      if (!t.startsWith("data:")) continue;
      const payload = t.slice(5).trim();
      if (!payload || payload === "[DONE]") continue;
      try {
        const chunk = JSON.parse(payload);
        id = chunk.id || id;
        const choice = chunk.choices?.[0];
        const delta = choice?.delta || {};
        if (typeof delta.content === "string") content += delta.content;
        if (Array.isArray(delta.tool_calls)) {
          for (const tc of delta.tool_calls) {
            const idx = tc.index ?? toolCalls.length;
            if (!toolCalls[idx]) {
              toolCalls[idx] = {
                id: tc.id || `call_${idx}`,
                type: "function",
                function: { name: "", arguments: "" },
              };
            }
            if (tc.id) toolCalls[idx].id = tc.id;
            if (tc.function?.name) toolCalls[idx].function.name += tc.function.name;
            if (tc.function?.arguments) toolCalls[idx].function.arguments += tc.function.arguments;
          }
        }
        if (choice?.finish_reason) finish = choice.finish_reason;
        if (chunk.usage) {
          usage = {
            prompt_tokens: chunk.usage.prompt_tokens || usage.prompt_tokens,
            completion_tokens: chunk.usage.completion_tokens || usage.completion_tokens,
            total_tokens: chunk.usage.total_tokens || usage.total_tokens,
          };
        }
      } catch {}
    }
  }

  const message: any = { role: "assistant", content: content || null };
  if (toolCalls.length) message.tool_calls = toolCalls;

  return {
    id,
    object: "chat.completion",
    created: Math.floor(Date.now() / 1000),
    model,
    choices: [{ index: 0, message, finish_reason: finish }],
    usage,
  };
}

async function upstreamChat(account: AccountTok, body: any, wantClientStream: boolean) {
  const upstreamBody = {
    ...body,
    stream: true, // ALWAYS stream to CodeBuddy
    messages: injectSystem(body.messages || []),
    model: resolveModel(body.model || "cb-haiku-4.5"),
  };

  const res = await fetch(UPSTREAM, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "text/event-stream, application/json, */*",
      Authorization: `Bearer ${account.access_token}`,
      "X-Requested-With": "XMLHttpRequest",
      "User-Agent":
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
    },
    body: JSON.stringify(upstreamBody),
    signal: AbortSignal.timeout(300_000),
  });

  if (res.status === 401 || res.status === 403) {
    markFail(account);
    return Response.json(
      { error: { message: `CodeBuddy auth failed for ${account.email}`, type: "auth_error" } },
      { status: 503 }
    );
  }
  if (res.status === 429) {
    markFail(account);
    return Response.json(
      { error: { message: `CodeBuddy quota/rate limit for ${account.email}`, type: "rate_limit" } },
      { status: 429 }
    );
  }
  if (!res.ok) {
    const text = await res.text();
    // 11101 etc — cooldown this account briefly
    if (text.includes("11101") || res.status >= 500) markFail(account);
    return new Response(text, {
      status: res.status,
      headers: { "Content-Type": res.headers.get("content-type") || "application/json" },
    });
  }

  markOk(account);

  if (wantClientStream) {
    // pass through SSE
    return new Response(res.body, {
      status: 200,
      headers: {
        "Content-Type": "text/event-stream; charset=utf-8",
        "Cache-Control": "no-cache",
        Connection: "keep-alive",
        "X-Account-Email": account.email,
      },
    });
  }

  // buffer SSE -> JSON
  const json = await aggregateSse(res, upstreamBody.model);
  return Response.json(json, {
    headers: { "X-Account-Email": account.email },
  });
}

const server = Bun.serve({
  hostname: HOST,
  port: PORT,
  async fetch(req) {
    const url = new URL(req.url);
    const path = url.pathname.replace(/\/+$/, "") || "/";

    if (req.method === "GET" && (path === "/health" || path === "/api/health")) {
      const ready = accounts.filter((a) => a.cooldownUntil <= Date.now()).length;
      return Response.json({
        ok: true,
        service: "codebuddy-proxy",
        accounts: accounts.length,
        ready,
        upstream: UPSTREAM,
      });
    }

    if (req.method === "POST" && path === "/admin/reload") {
      accounts = loadAccounts();
      rr = 0;
      return Response.json({ reloaded: accounts.length });
    }

    if (!authOk(req)) {
      return Response.json({ error: { message: "Unauthorized", type: "auth_error" } }, { status: 401 });
    }

    if (req.method === "GET" && (path === "/v1/models" || path === "/models")) {
      return Response.json({ object: "list", data: MODELS });
    }

    if (
      req.method === "POST" &&
      (path === "/v1/chat/completions" || path === "/chat/completions")
    ) {
      let body: any;
      try {
        body = await req.json();
      } catch {
        return Response.json({ error: { message: "Invalid JSON body" } }, { status: 400 });
      }

      const wantStream = Boolean(body.stream);
      // try up to 5 accounts on auth/rate failures
      let last: Response | null = null;
      for (let attempt = 0; attempt < 5; attempt++) {
        const acc = pickAccount();
        if (!acc) {
          return Response.json(
            {
              error: {
                message: `No ready CodeBuddy accounts (total=${accounts.length})`,
                type: "server_error",
              },
            },
            { status: 503 }
          );
        }
        last = await upstreamChat(acc, body, wantStream);
        // retry only on 503 auth/rate from our wrapper
        if (last.status !== 503 && last.status !== 429) return last;
      }
      return last || Response.json({ error: { message: "All accounts failed" } }, { status: 503 });
    }

    return Response.json({ error: { message: `Not found: ${path}` } }, { status: 404 });
  },
});

console.log(`[codebuddy-proxy] http://${HOST}:${PORT}`);
console.log(`[codebuddy-proxy] accounts loaded: ${accounts.length}`);
console.log(`[codebuddy-proxy] key: ${PROXY_KEY}`);
console.log(`[codebuddy-proxy] point 9router baseUrl to http://${HOST}:${PORT}/v1`);
