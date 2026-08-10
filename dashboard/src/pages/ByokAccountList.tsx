import { useEffect, useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { ArrowLeft, Eye, EyeOff, FlaskConical, Key, Plus, RefreshCw, Save, Trash2, Zap, Download, Loader2, ListChecks, Copy } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle as DTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import {
  deleteAccount,
  fetchByokProviders,
  importByokModels,
  revealByokKey,
  testByokProvider,
  toggleAccountEnabled,
  updateByokProvider,
  type ByokKeyInfo,
  type ByokProvider,
} from "@/lib/api";
import { formatDateTimeID } from "@/lib/utils";
import { useTimedMessage } from "@/hooks/useTimedMessage";
import { useWsEvent } from "@/hooks/useWebSocket";

type LbMethod = "round_robin" | "sequential" | "least_inflight";
type ApiFormat = "openai" | "anthropic" | "auto";

type KeyDraft = {
  id?: number;
  label: string;
  key: string;
  enabled: boolean;
  status?: string;
  errorMessage?: string | null;
};

const MASK = "••••••••";

function emptyKey(index = 0): KeyDraft {
  return { label: index === 0 ? "default" : `key-${index + 1}`, key: "", enabled: true };
}

function formatDate(value?: string | null) {
  if (!value) return "-";
  return formatDateTimeID(value);
}

function lbLabel(method?: string) {
  if (method === "sequential") return "Sequential";
  if (method === "least_inflight") return "Least in-flight";
  return "Round Robin";
}

export default function ByokAccountList() {
  const { prefix } = useParams<{ prefix: string }>();
  const navigate = useNavigate();
  const [provider, setProvider] = useState<ByokProvider | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [testingKey, setTestingKey] = useState<number | null>(null);
  const [revealingKey, setRevealingKey] = useState<string | null>(null);
  const [visibleSecrets, setVisibleSecrets] = useState<Set<string>>(new Set());
  const [importingModels, setImportingModels] = useState(false);
  const [importPickerOpen, setImportPickerOpen] = useState(false);
  const [importCandidates, setImportCandidates] = useState<string[]>([]);
  const [importSelected, setImportSelected] = useState<Set<string>>(new Set());
  const [testingModel, setTestingModel] = useState<string | null>(null);
  const { message, setMessage, clearMessage } = useTimedMessage<string>(null, 4000);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState({
    base_url: "",
    format: "auto" as ApiFormat,
    load_balancing_method: "round_robin" as LbMethod,
    models: "",
    keys: [emptyKey()] as KeyDraft[],
  });

  function showSuccess(text: string) { setMessage(text); setError(null); }
    function showError(err: unknown) { setError(err instanceof Error ? err.message : String(err)); clearMessage(); }

    async function handleImportModels() {
      if (!form.base_url) {
        showError(new Error("Base URL is required to import models"));
        return;
      }
      try {
        const entered = new URL(form.base_url.trim());
        if (entered.hostname === "localhost" && entered.port === "1931") {
          showError(new Error("1931 is the Luminus dashboard. Use the API backend at http://localhost:1930/v1 for proxy models."));
          return;
        }
      } catch {
        showError(new Error("Base URL must be a valid http:// or https:// URL"));
        return;
      }
      const firstKeyWithSecret = form.keys.find((k) => k.key && k.key !== MASK && k.key.trim().length > 0);
      const apiKey = firstKeyWithSecret ? firstKeyWithSecret.key.trim() : "";
      if (!apiKey) {
        showError(new Error("Add a valid API key above to authenticate the import request"));
        return;
      }

      setImportingModels(true);
      try {
        const res = await importByokModels({
          base_url: form.base_url.trim(),
          api_key: apiKey,
        });
        if (!res.success) {
          throw new Error(res.error || "Failed to fetch models");
        }
        if (res.models.length === 0) {
          showSuccess("No models returned from provider");
          return;
        }
        const ids = res.models.map((m) => m.id);
        setImportCandidates(ids);
        setImportSelected(new Set(ids));
        setImportPickerOpen(true);
      } catch (err) {
        showError(err);
      } finally {
        setImportingModels(false);
      }
    }

    async function handleConfirmImport() {
      const selected = Array.from(importSelected);
      if (selected.length === 0) {
        showError(new Error("Please select at least one model to import"));
        return;
      }
      const existing = form.models.split(",").map((m) => m.trim()).filter(Boolean);
      const combined = Array.from(new Set([...existing, ...selected]));
      setForm((current) => ({ ...current, models: combined.join(", ") }));
      setImportPickerOpen(false);
      showSuccess(`Imported ${selected.length} model(s) — auto-saving...`);
      // Auto-save so models persist across refreshes without requiring the user
      // to manually click the top-right Save Settings button.
      try {
        await saveSettings(combined);
      } catch (err) {
        showError(err);
      }
    }

    async function load() {
    if (!prefix) return;
    setLoading(true);
    try {
      const res = await fetchByokProviders();
      const found = (res.providers || []).find((p) => p.label === prefix);
      if (!found) {
        setProvider(null);
        setError(`BYOK provider "${prefix}" not found`);
        return;
      }
      setProvider(found);
      setForm({
        base_url: found.base_url || "",
        format: found.format || "auto",
        load_balancing_method: found.load_balancing_method || "round_robin",
        models: (found.models || []).join(", "),
        keys: (found.keys && found.keys.length > 0)
          ? found.keys.map((key) => ({
              id: key.id,
              label: key.label,
              key: MASK,
              enabled: key.enabled !== false,
              status: key.status,
              errorMessage: key.errorMessage,
            }))
          : [emptyKey()],
      });
    } catch (err) {
      showError(err);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { load(); }, [prefix]);
  useWsEvent(["byok_created", "byok_updated", "byok_deleted", "account_status", "account_deleted"], load);

  const models = useMemo(() => form.models.split(",").map((m) => m.trim()).filter(Boolean), [form.models]);
  const activeKeyCount = form.keys.filter((k) => k.enabled && k.status !== "error").length;

  function secretVisibilityId(key: KeyDraft, index: number) {
    return key.id ? `id-${key.id}` : `new-${index}`;
  }

  async function toggleSecretVisibility(key: KeyDraft, index: number) {
    const visibilityId = secretVisibilityId(key, index);
    const isVisible = visibleSecrets.has(visibilityId);

    if (isVisible) {
      setVisibleSecrets((current) => {
        const next = new Set(current);
        next.delete(visibilityId);
        return next;
      });
      return;
    }

    if (key.id && key.key === MASK) {
      setRevealingKey(visibilityId);
      try {
        const revealed = await revealByokKey(key.id);
        updateKey(index, { key: revealed.key });
      } catch (err) {
        showError(err);
        setRevealingKey(null);
        return;
      }
      setRevealingKey(null);
    }

    setVisibleSecrets((current) => {
      const next = new Set(current);
      next.add(visibilityId);
      return next;
    });
  }

  function updateKey(index: number, patch: Partial<KeyDraft>) {
    setForm((current) => ({
      ...current,
      keys: current.keys.map((key, i) => i === index ? { ...key, ...patch } : key),
    }));
  }

  function addKey() {
    setForm((current) => ({ ...current, keys: [...current.keys, emptyKey(current.keys.length)] }));
  }

  async function removeKey(index: number) {
    const key = form.keys[index];
    if (!key) return;
    if (key.id) {
      if (!confirm(`Delete API key "${key.label}"?`)) return;
      try {
        await deleteAccount(key.id);
        showSuccess(`Deleted key ${key.label}`);
        await load();
      } catch (err) { showError(err); }
      return;
    }
    setForm((current) => ({
      ...current,
      keys: current.keys.length <= 1 ? [emptyKey()] : current.keys.filter((_, i) => i !== index),
    }));
  }

  function buildPayloadKeys() {
    return form.keys.map((key, index) => ({
      id: key.id,
      label: key.label.trim().toLowerCase() || `key-${index + 1}`,
      key: key.key && key.key !== MASK ? key.key.trim() : undefined,
      enabled: key.enabled,
      priority: index,
    })).filter((key) => key.id || key.key);
  }

  async function saveSettings(overrideModels?: string[]) {
    if (!provider) return;
    if (!form.base_url.trim()) return showError(new Error("Base URL is required"));
    const modelsToSave = overrideModels ?? models;
    if (modelsToSave.length === 0) return showError(new Error("At least one model is required"));
    const apiKeys = buildPayloadKeys();
    if (apiKeys.length === 0) return showError(new Error("At least one API key is required"));

    setSaving(true);
    try {
      await updateByokProvider(provider.id, {
        base_url: form.base_url.trim(),
        format: form.format,
        load_balancing_method: form.load_balancing_method,
        models: modelsToSave,
        api_keys: apiKeys,
      });
      showSuccess("BYOK provider saved");
      await load();
    } catch (err) {
      showError(err);
    } finally {
      setSaving(false);
    }
  }

  async function toggleKey(key: KeyDraft, index: number) {
    const next = !key.enabled;
    updateKey(index, { enabled: next });
    if (!key.id) return;
    try {
      await toggleAccountEnabled(key.id, next);
      showSuccess(next ? `Enabled ${key.label}` : `Disabled ${key.label}`);
      await load();
    } catch (err) {
      updateKey(index, { enabled: key.enabled });
      showError(err);
    }
  }

  async function testKey(key: KeyDraft) {
    if (!key.id) return showError(new Error("Save this key before testing"));
    setTestingKey(key.id);
    try {
      const res = await testByokProvider(key.id);
      if (res.success) showSuccess(`✓ ${key.label} OK${res.latency_ms ? ` · ${res.latency_ms}ms` : ""}`);
      else showError(new Error(res.error || "Connection test failed"));
      await load();
    } catch (err) {
      showError(err);
    } finally {
      setTestingKey(null);
    }
  }

  /**
   * Test a single model by routing a minimal request through the provider's
   * first active API key. Uses the optional `model` field accepted by the
   * /byok/:id/test endpoint.
   */
  async function testModel(modelId: string) {
    const firstActiveKey = form.keys.find((k) => k.id && k.enabled);
    if (!firstActiveKey) {
      showError(new Error("Save and enable at least one API key before testing models"));
      return;
    }
    setTestingModel(modelId);
    try {
      const res = await testByokProvider(firstActiveKey.id!, modelId);
      if (res.success) {
        showSuccess(`✓ ${modelId} OK${res.latency_ms ? ` · ${res.latency_ms}ms` : ""}`);
      } else {
        showError(new Error(res.error || `Test failed for ${modelId}`));
      }
    } catch (err) {
      showError(err);
    } finally {
      setTestingModel(null);
    }
  }

  function removeModel(modelId: string) {
    const next = models.filter((m) => m !== modelId);
    setForm({ ...form, models: next.join(", ") });
    showSuccess(`Removed ${modelId}`);
  }

  async function testAll() {
    for (const key of form.keys) {
      if (key.id) await testKey(key);
    }
  }

  if (loading && !provider) {
    return <div className="flex h-64 items-center justify-center text-sm text-[var(--muted-foreground)]">Loading BYOK provider...</div>;
  }

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="icon" onClick={() => navigate("/accounts")}>
            <ArrowLeft className="w-5 h-5" />
          </Button>
          <div>
            <h1 className="text-2xl font-bold text-[var(--foreground)]">BYOK · {prefix}</h1>
            <p className="text-sm text-[var(--muted-foreground)] mt-1">
              {form.keys.length} keys · {activeKeyCount} enabled · {models.length} models · {lbLabel(form.load_balancing_method)}
            </p>
          </div>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" onClick={load} disabled={loading}>
            <RefreshCw className="w-4 h-4 mr-2" /> Refresh
          </Button>
          <Button variant="outline" size="sm" onClick={testAll} disabled={testingKey !== null || form.keys.every((k) => !k.id)}>
            <FlaskConical className="w-4 h-4 mr-2" /> Test All
          </Button>
          <Button size="sm" onClick={() => saveSettings()} disabled={saving}>
            <Save className="w-4 h-4 mr-2" /> {saving ? "Saving..." : "Save Settings"}
          </Button>
        </div>
      </div>

      {(message || error) && (
        <div className={`rounded-md p-3 text-sm ${message ? "bg-[var(--success)]/10 text-[var(--success)]" : "bg-[var(--error)]/10 text-[var(--error)]"}`}>
          {message || error}
        </div>
      )}

      <Card className="border-[var(--border)]">
        <CardHeader>
          <CardTitle className="text-base">Provider Settings</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid gap-3 md:grid-cols-2">
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-[var(--foreground)]">Provider Prefix</label>
              <Input value={prefix || ""} readOnly className="font-mono bg-[var(--muted)] opacity-70" />
            </div>
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-[var(--foreground)]">Base URL</label>
              <Input value={form.base_url} onChange={(e) => setForm({ ...form, base_url: e.target.value })} placeholder="https://api.provider.com/v1" />
            </div>
          </div>
          <div className="grid gap-3 md:grid-cols-2">
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-[var(--foreground)]">API Format</label>
              <select value={form.format} onChange={(e) => setForm({ ...form, format: e.target.value as ApiFormat })} className="w-full h-9 rounded-md border border-[var(--border)] bg-[var(--background)] px-3 text-sm text-[var(--foreground)]">
                <option value="auto">Auto-detect</option>
                <option value="openai">OpenAI-compatible</option>
                <option value="anthropic">Anthropic</option>
              </select>
            </div>
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-[var(--foreground)]">Load Balancing</label>
              <select value={form.load_balancing_method} onChange={(e) => setForm({ ...form, load_balancing_method: e.target.value as LbMethod })} className="w-full h-9 rounded-md border border-[var(--border)] bg-[var(--background)] px-3 text-sm text-[var(--foreground)]">
                <option value="round_robin">Round Robin</option>
                <option value="sequential">Sequential</option>
              </select>
              <p className="text-xs text-[var(--muted-foreground)]">Round Robin rotates keys. Sequential prioritizes the first healthy key in table order.</p>
            </div>
          </div>
          {/* Available Models — grid card layout matching Cartethyia reference */}
          <div className="space-y-3 pt-2 border-t border-[var(--border)]">
            <div className="flex items-start justify-between gap-3 pt-3">
              <div className="space-y-1">
                <div className="flex items-center gap-2">
                  <ListChecks className="w-4 h-4 text-[var(--success)]" />
                  <label className="text-sm font-semibold text-[var(--foreground)]">Available Models</label>
                  <Badge variant="secondary" className="text-[10px]">{models.length}</Badge>
                </div>
                <p className="text-xs text-[var(--muted-foreground)]">
                  Discovered via a live GET /models call — routing accepts any model id regardless of this list.
                </p>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="gap-2 shrink-0"
                onClick={handleImportModels}
                disabled={importingModels}
                title="Fetch model IDs from the provider's /models endpoint using the base URL and first API key above. The API key is not stored server-side beyond this request."
              >
                {importingModels ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <RefreshCw className="w-4 h-4" />
                )}
                Fetch models
              </Button>
            </div>

            {/* Model cards grid */}
            {models.length === 0 ? (
              <div className="rounded-md border border-dashed border-[var(--border)] bg-[var(--secondary)]/30 p-8 text-center">
                <p className="text-sm text-[var(--muted-foreground)]">
                  No models configured yet. Click <span className="font-medium text-[var(--foreground)]">Fetch models</span> or add comma-separated IDs below.
                </p>
              </div>
            ) : (
              <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-2">
                {models.map((modelId) => (
                  <div
                    key={modelId}
                    className="group rounded-md border border-[var(--border)] bg-[var(--secondary)]/40 hover:bg-[var(--secondary)]/70 hover:border-[var(--primary)]/40 transition-all p-3 space-y-2 relative"
                  >
                    <div className="flex items-start justify-between gap-2">
                      <div className="min-w-0 flex-1">
                        <div className="font-mono text-xs text-[var(--foreground)] truncate font-medium" title={modelId}>
                          {modelId}
                        </div>
                        <div className="font-mono text-[10px] text-[var(--muted-foreground)] truncate mt-0.5" title={`${prefix}/${modelId}`}>
                          {prefix}/{modelId}
                        </div>
                      </div>
                      <button
                        type="button"
                        onClick={() => {
                          navigator.clipboard?.writeText(`${prefix}/${modelId}`).catch(() => {});
                          showSuccess(`Copied ${prefix}/${modelId}`);
                        }}
                        className="p-1 rounded text-[var(--muted-foreground)] hover:text-[var(--foreground)] hover:bg-[var(--background)] opacity-0 group-hover:opacity-100 transition-opacity"
                        title="Copy public model id"
                        aria-label="Copy model id"
                      >
                        <Copy className="w-3 h-3" />
                      </button>
                    </div>
                    <div className="flex items-center gap-1.5">
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        className="h-7 flex-1 gap-1 text-xs"
                        onClick={() => testModel(modelId)}
                        disabled={testingModel === modelId || form.keys.every((k) => !k.id)}
                        title="Send a minimal test request to this model"
                      >
                        {testingModel === modelId ? (
                          <Loader2 className="w-3 h-3 animate-spin" />
                        ) : (
                          <FlaskConical className="w-3 h-3 text-[var(--info)]" />
                        )}
                        Test
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7 shrink-0 text-[var(--muted-foreground)] hover:text-[var(--error)] hover:bg-[var(--error)]/10"
                        onClick={() => removeModel(modelId)}
                        title="Remove model"
                        aria-label="Remove model"
                      >
                        <Trash2 className="w-3 h-3" />
                      </Button>
                    </div>
                  </div>
                ))}
              </div>
            )}

            {/* Manual entry — collapsible raw textarea */}
            <details className="group">
              <summary className="cursor-pointer text-xs text-[var(--muted-foreground)] hover:text-[var(--foreground)] transition-colors select-none py-1">
                Raw editor (comma-separated model IDs)
              </summary>
              <textarea
                value={form.models}
                onChange={(e) => setForm({ ...form, models: e.target.value })}
                className="mt-2 w-full h-20 rounded-md border border-[var(--border)] bg-[var(--background)] p-3 text-sm font-mono text-[var(--foreground)]"
                placeholder="gpt-4o, claude-sonnet, llama-3"
              />
              <p className="text-xs text-[var(--muted-foreground)] mt-1">
                Public model IDs become <span className="font-mono">{prefix || "prefix"}-model</span>.
              </p>
            </details>
          </div>
        </CardContent>
      </Card>

      <Card className="border-[var(--border)]">
        <CardHeader className="flex flex-row items-center justify-between space-y-0">
          <CardTitle className="text-base">API Keys</CardTitle>
          <Button variant="outline" size="sm" onClick={addKey}>
            <Plus className="w-4 h-4 mr-2" /> Add Key
          </Button>
        </CardHeader>
        <CardContent className="p-0">
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b border-[var(--border)]">
                  <th className="p-4 text-left text-xs font-medium uppercase tracking-wide text-[var(--muted-foreground)]">Key Label</th>
                  <th className="p-4 text-left text-xs font-medium uppercase tracking-wide text-[var(--muted-foreground)]">Secret</th>
                  <th className="p-4 text-left text-xs font-medium uppercase tracking-wide text-[var(--muted-foreground)]">Status</th>
                  <th className="p-4 text-left text-xs font-medium uppercase tracking-wide text-[var(--muted-foreground)]">Enabled</th>
                  <th className="p-4 text-left text-xs font-medium uppercase tracking-wide text-[var(--muted-foreground)]">Last Used</th>
                  <th className="p-4 text-left text-xs font-medium uppercase tracking-wide text-[var(--muted-foreground)]">Actions</th>
                </tr>
              </thead>
              <tbody>
                {form.keys.map((key, index) => {
                  const visibilityId = secretVisibilityId(key, index);
                  const secretVisible = visibleSecrets.has(visibilityId);
                  return (
                  <tr key={`${key.id || "new"}-${index}`} className="border-b border-[var(--border)] last:border-0 hover:bg-[var(--secondary)]/40">
                    <td className="p-4">
                      <Input value={key.label} onChange={(e) => updateKey(index, { label: e.target.value })} className="h-8 min-w-[140px] font-mono text-xs" />
                      {form.load_balancing_method === "sequential" && <div className="mt-1 text-[10px] text-[var(--muted-foreground)]">Priority #{index + 1}</div>}
                    </td>
                    <td className="p-4">
                      <div className="flex min-w-[260px] items-center gap-1">
                        <Input
                          type={secretVisible ? "text" : "password"}
                          value={key.key}
                          onChange={(e) => updateKey(index, { key: e.target.value })}
                          onFocus={() => { if (key.key === MASK) updateKey(index, { key: "" }); }}
                          placeholder={key.id ? "Keep masked or paste new key" : "sk-..."}
                          className="h-8 font-mono text-xs"
                        />
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 shrink-0"
                          onClick={() => toggleSecretVisibility(key, index)}
                          disabled={revealingKey === visibilityId}
                          title={secretVisible ? "Hide key" : "Show key"}
                        >
                          {revealingKey === visibilityId ? <RefreshCw className="h-4 w-4 animate-spin" /> : secretVisible ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                        </Button>
                      </div>
                    </td>
                    <td className="p-4">
                      <Badge variant={key.status === "error" ? "error" : key.status === "active" ? "success" : "secondary"}>{key.status || (key.id ? "active" : "new")}</Badge>
                      {key.errorMessage && <div className="mt-1 max-w-[220px] truncate text-xs text-[var(--error)]" title={key.errorMessage}>{key.errorMessage}</div>}
                    </td>
                    <td className="p-4">
                      <button type="button" onClick={() => toggleKey(key, index)} className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${key.enabled ? "bg-[var(--success)]" : "bg-[var(--secondary)]"}`}>
                        <span className={`inline-block h-4 w-4 rounded-full bg-white transition-transform ${key.enabled ? "translate-x-4" : "translate-x-0.5"}`} />
                      </button>
                    </td>
                    <td className="p-4 text-xs text-[var(--muted-foreground)]">{formatDate((provider?.keys || []).find((k: ByokKeyInfo) => k.id === key.id)?.lastUsedAt)}</td>
                    <td className="p-4">
                      <div className="flex gap-1">
                        <Button variant="ghost" size="icon" onClick={() => testKey(key)} disabled={testingKey === key.id || !key.id} title="Test key">
                          {testingKey === key.id ? <RefreshCw className="w-4 h-4 animate-spin text-[var(--info)]" /> : <Zap className="w-4 h-4 text-[var(--info)]" />}
                        </Button>
                        <Button variant="ghost" size="icon" onClick={() => removeKey(index)} title="Delete key">
                          <Trash2 className="w-4 h-4 text-[var(--error)]" />
                        </Button>
                      </div>
                    </td>
                  </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </CardContent>
      </Card>

      {/* Import Models Picker Dialog */}
      <Dialog open={importPickerOpen} onOpenChange={setImportPickerOpen}>
        <DialogContent className="max-w-md max-h-[85vh] overflow-y-auto border-[var(--border)] bg-[var(--card)]">
          <DialogHeader>
            <DTitle className="text-lg font-bold text-[var(--foreground)] flex items-center gap-2">
              <ListChecks className="w-5 h-5 text-[var(--primary)]" />
              Select Models to Import
            </DTitle>
            <DialogDescription className="text-xs text-[var(--muted-foreground)]">
              Choose which models from the provider you want to configure.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 pt-2">
            <div className="flex items-center justify-between border-b border-[var(--border)] pb-2">
              <span className="text-xs text-[var(--muted-foreground)] font-medium">
                {importCandidates.length} models found
              </span>
              <div className="flex gap-2">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7 text-xs text-[var(--primary)] hover:bg-[var(--secondary)]"
                  onClick={() => setImportSelected(new Set(importCandidates))}
                >
                  Select All
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7 text-xs text-[var(--muted-foreground)] hover:bg-[var(--secondary)]"
                  onClick={() => setImportSelected(new Set())}
                >
                  Clear All
                </Button>
              </div>
            </div>

            <div className="space-y-2 max-h-60 overflow-y-auto pr-1">
              {importCandidates.map((modelId) => {
                const isSelected = importSelected.has(modelId);
                return (
                  <label
                    key={modelId}
                    className="flex items-center justify-between p-2 rounded-md border border-[var(--border)] bg-[var(--secondary)]/40 hover:bg-[var(--secondary)]/70 cursor-pointer transition-colors text-sm"
                  >
                    <span className="font-mono text-xs text-[var(--foreground)] truncate select-all">
                      {modelId}
                    </span>
                    <button
                      type="button"
                      onClick={(e) => {
                        e.preventDefault();
                        const next = new Set(importSelected);
                        if (next.has(modelId)) {
                          next.delete(modelId);
                        } else {
                          next.add(modelId);
                        }
                        setImportSelected(next);
                      }}
                      className={`relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors ${isSelected ? "bg-[var(--primary)]" : "bg-[var(--border)]"}`}
                    >
                      <span className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white transition-transform ${isSelected ? "translate-x-5" : "translate-x-1"}`} />
                    </button>
                  </label>
                );
              })}
            </div>

            <div className="flex justify-end gap-2 pt-2 border-t border-[var(--border)]">
              <Button
                variant="outline"
                onClick={() => setImportPickerOpen(false)}
                className="text-[var(--muted-foreground)]"
              >
                Cancel
              </Button>
              <Button onClick={handleConfirmImport} className="shadow-sm">
                Confirm Import
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
