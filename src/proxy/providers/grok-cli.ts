import { BaseProvider, type ChatCompletionRequest, type ModelInfo, type ProviderResult } from "./base";
import type { Account } from "../../db/schema";

const BASE = "https://cli-chat-proxy.grok.com/v1";
const MODELS: ModelInfo[] = [
  { id: "gcli/grok-imagine-image", object: "model", created: Date.now(), owned_by: "grok-cli", vision: true, creditUnit: "request", creditSource: "upstream" },
  { id: "gcli/grok-image", object: "model", created: Date.now(), owned_by: "grok-cli", vision: true, creditUnit: "request", creditSource: "upstream" },
  { id: "gcli/grok-build", object: "model", created: Date.now(), owned_by: "grok-cli", thinking: true, vision: true, creditUnit: "credit", creditSource: "upstream" },
  { id: "gcli/grok-4.5", object: "model", created: Date.now(), owned_by: "grok-cli", thinking: true, vision: true, creditUnit: "credit", creditSource: "upstream" },
  { id: "gcli/grok-4.5-high", object: "model", created: Date.now(), owned_by: "grok-cli", thinking: true, vision: true, creditUnit: "credit", creditSource: "upstream" },
  { id: "gcli/grok-4.5-medium", object: "model", created: Date.now(), owned_by: "grok-cli", thinking: true, vision: true, creditUnit: "credit", creditSource: "upstream" },
  { id: "gcli/grok-4.5-low", object: "model", created: Date.now(), owned_by: "grok-cli", thinking: true, vision: true, creditUnit: "credit", creditSource: "upstream" },
];

export class GrokCliProvider extends BaseProvider {
  name = "grok-cli";
  supportedModels = MODELS;
  override ownsModel(model: string) { return model.toLowerCase().startsWith("gcli/"); }
  private token(account: Account) {
    const raw: any = account.tokens;
    const t = typeof raw === "string" ? JSON.parse(raw) : raw;
    const value = t?.access_token || t?.accessToken || t?.api_key;
    return value || null;
  }
  async chatCompletion(account: Account, request: ChatCompletionRequest): Promise<ProviderResult> {
    const token = this.token(account);
    if (!token) return { success: false, error: "Grok CLI OAuth token is missing" };
    const upstream = request.model.replace(/^gcli\//i, "");
    const res = await fetch(`${BASE}/chat/completions`, { method: "POST", headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json", Accept: "application/json", "x-xai-token-auth": "xai-grok-cli", "x-grok-client-identifier": "grok-pager" }, body: JSON.stringify({ ...request, model: upstream }) });
    if (!res.ok) return { success: false, error: `Grok CLI returned ${res.status}: ${(await res.text()).slice(0, 500)}` };
    const response = await res.json() as any;
    return { success: true, response, tokensUsed: response.usage?.total_tokens || 0 };
  }
  async chatCompletionStream(account: Account, request: ChatCompletionRequest): Promise<ProviderResult> { return this.chatCompletion(account, { ...request, stream: false }); }
  async refreshToken(account: Account) { return { success: false, error: "Re-authorize Grok CLI through 9router" }; }
  async validateAccount(account: Account) { try { return !!this.token(account); } catch { return false; } }
  async fetchQuota() { return { success: false, error: "Grok CLI quota is managed by 9router" }; }
}

export { MODELS as grokCliModels };

export default GrokCliProvider;

// Native OAuth account. This provider is intentionally not BYOK.
// OAuth authorization is performed by 9router; the resulting access token is imported encrypted.
