import {
  BaseProvider,
  type ChatCompletionRequest,
  type ChatCompletionResponse,
  type ModelInfo,
  type ProviderHealthResult,
  type ProviderResult,
  type StreamChunk,
} from "./base";
import type { Account } from "../../db/schema";
import { config } from "../../config";

/**
 * Blackbox Global model catalog.
 * Exposed as bb/<upstream-id>; Blackbox upstream receives the raw id.
 */
const BB_MODEL_MAP: Record<string, string> = {
  "bb/claude-fable-5": "claude-fable-5",
  "bb/claude-opus-4.8": "claude-opus-4.8",
  "bb/claude-sonnet-4.6": "claude-sonnet-4.6",
  "bb/gpt-5.5": "gpt-5.5",
  "bb/gpt-5.4-pro": "gpt-5.4-pro",
  "bb/gpt-5.4": "gpt-5.4",
  "bb/gpt-5.3-codex": "gpt-5.3-codex",
  "bb/gpt-5.4-nano": "gpt-5.4-nano",
  "bb/deepseek-v4-flash": "deepseek-v4-flash",
  "bb/grok-4.3": "grok-4.3",
};

export class BlackboxProvider extends BaseProvider {
  name = "blackbox";

  override ownsModel(model: string): boolean {
    if (!model) return false;
    const lower = model.toLowerCase();
    return lower.startsWith("bb-") || lower.startsWith("bb/") || lower.startsWith("bb_");
  }

  private resolveModel(model: string): string {
    let m = (model || "").trim();
    if (m.includes("/")) m = m.split("/").pop() || m;
    let lower = m.toLowerCase();

    if (lower.includes("_")) {
      lower = lower.replace(/_/g, "-").replace(/(\d+)-(\d+)$/, "$1.$2");
      m = lower;
    }

    const isThinking = m.endsWith("-thinking");
    const base = isThinking ? m.replace(/-thinking$/, "") : m;
    const resolved = BB_MODEL_MAP[base.toLowerCase()] || base;
    return isThinking ? `${resolved}-thinking` : resolved;
  }

  private baseUrl = "https://api.blackbox.ai/v1";

  supportedModels: ModelInfo[] = [
    { id: "bb/claude-fable-5", object: "model", created: Date.now(), owned_by: "blackbox", context_window: 200000, max_output: 8192, thinking: true, vision: true, creditUnit: "token", creditRate: 0, creditSource: "estimated" },
    { id: "bb/claude-opus-4.8", object: "model", created: Date.now(), owned_by: "blackbox", context_window: 200000, max_output: 8192, thinking: true, vision: true, creditUnit: "token", creditRate: 0, creditSource: "estimated" },
    { id: "bb/claude-sonnet-4.6", object: "model", created: Date.now(), owned_by: "blackbox", context_window: 200000, max_output: 8192, thinking: true, vision: true, creditUnit: "token", creditRate: 0, creditSource: "estimated" },
    { id: "bb/gpt-5.5", object: "model", created: Date.now(), owned_by: "blackbox", context_window: 200000, max_output: 8192, thinking: true, vision: true, creditUnit: "token", creditRate: 0, creditSource: "estimated" },
    { id: "bb/gpt-5.4-pro", object: "model", created: Date.now(), owned_by: "blackbox", context_window: 200000, max_output: 8192, thinking: true, vision: true, creditUnit: "token", creditRate: 0, creditSource: "estimated" },
    { id: "bb/gpt-5.4", object: "model", created: Date.now(), owned_by: "blackbox", context_window: 200000, max_output: 8192, thinking: true, vision: true, creditUnit: "token", creditRate: 0, creditSource: "estimated" },
    { id: "bb/gpt-5.3-codex", object: "model", created: Date.now(), owned_by: "blackbox", context_window: 200000, max_output: 8192, thinking: true, vision: false, creditUnit: "token", creditRate: 0, creditSource: "estimated" },
    { id: "bb/gpt-5.4-nano", object: "model", created: Date.now(), owned_by: "blackbox", context_window: 200000, max_output: 8192, thinking: true, vision: true, creditUnit: "token", creditRate: 0, creditSource: "estimated" },
    { id: "bb/deepseek-v4-flash", object: "model", created: Date.now(), owned_by: "blackbox", context_window: 200000, max_output: 8192, thinking: true, vision: false, creditUnit: "token", creditRate: 0, creditSource: "estimated" },
    { id: "bb/grok-4.3", object: "model", created: Date.now(), owned_by: "blackbox", context_window: 200000, max_output: 8192, thinking: true, vision: true, creditUnit: "token", creditRate: 0, creditSource: "estimated" },
  ];

  private getApiKey(account: Account): string | null {
    if (!account.tokens) return null;
    try {
      const t = typeof account.tokens === "string" ? JSON.parse(account.tokens) : account.tokens;
      return t.api_key || null;
    } catch {
      return null;
    }
  }

  async validateAccount(account: Account): Promise<boolean> {
    return this.getApiKey(account) !== null;
  }

  async refreshToken(account: Account): Promise<{ success: boolean; tokens?: string; error?: string }> {
    return { success: true }; // API keys don't refresh automatically
  }

  async fetchQuota(account: Account): Promise<{ success: boolean; quota?: any; error?: string }> {
    // We don't have a quota endpoint for Blackbox yet
    return { success: true, quota: { limit: -1, remaining: -1, used: 0 } };
  }

  override async healthCheck(account: Account): Promise<ProviderHealthResult> {
    const key = this.getApiKey(account);
    if (!key) return { kind: "missing_tokens", success: false, error: "Missing API Key" };
    try {
      const controller = new AbortController();
      const timeoutId = setTimeout(() => controller.abort(), 8000);
      const reqStart = performance.now();
      const res = await fetch(`${this.baseUrl}/models`, {
        method: "GET",
        headers: { "Authorization": `Bearer ${key}` },
        signal: controller.signal
      });
      clearTimeout(timeoutId);
      const reqDuration = Math.round(performance.now() - reqStart);
      
      if (!res.ok) {
        if (res.status === 401 || res.status === 403) return { kind: "auth_error", success: false, error: "Invalid API Key" };
        return { kind: "transient_error", success: false, error: `HTTP ${res.status}` };
      }
      return { kind: "healthy", success: true };
    } catch (e: any) {
      return { kind: "transient_error", success: false, error: e.message || "Unknown error" };
    }
  }

  async chatCompletion(account: Account, req: ChatCompletionRequest): Promise<ProviderResult> {
    const key = this.getApiKey(account);
    if (!key) throw new Error("Missing API Key");

    const upstreamReq = { ...req, model: this.resolveModel(req.model) };
    const res = await fetch(`${this.baseUrl}/chat/completions`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Authorization": `Bearer ${key}`
      },
      body: JSON.stringify(upstreamReq)
    });

    if (!res.ok) {
      const text = await res.text().catch(() => "");
      return { success: false, error: `HTTP ${res.status}: ${text}`, rateLimited: res.status === 429, quotaExhausted: res.status === 402 };
    }

    const data = await res.json();
    return { success: true, response: data as ChatCompletionResponse };
  }

  async chatCompletionStream(account: Account, req: ChatCompletionRequest): Promise<ProviderResult> {
    const key = this.getApiKey(account);
    if (!key) throw new Error("Missing API Key");

    const upstreamReq = { ...req, model: this.resolveModel(req.model), stream: true };
    const res = await fetch(`${this.baseUrl}/chat/completions`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Authorization": `Bearer ${key}`
      },
      body: JSON.stringify(upstreamReq)
    });

    if (!res.ok) {
      const text = await res.text().catch(() => "");
      return { success: false, error: `HTTP ${res.status}: ${text}`, rateLimited: res.status === 429, quotaExhausted: res.status === 402 };
    }

    if (!res.body) return { success: false, error: "No response body" };
    
    // We pass the raw SSE stream through to the proxy handler
    return { success: true, stream: res.body };
  }
}
