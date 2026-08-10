import { useEffect, useState } from "react";
import { Copy, Check, Trash2, Plus, Eye, EyeOff, RefreshCw, Power } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  fetchApiKey, regenerateApiKey, setApiKey,
  fetchPublicApiKeys as fetchCustomKeys,
  createPublicApiKey as createCustomKey,
  updatePublicApiKey as updateCustomKey,
  deletePublicApiKey as deleteCustomKey,
  fetchModels,
  type PublicApiKey as ApiKeyRow,
} from "@/lib/api";

function fmt(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function generateKey() {
  return "sk-" + crypto.randomUUID().replace(/-/g, "").slice(0, 24);
}

// ── Admin key section ────────────────────────────────────────────────────────
function AdminKeySection() {
  const [key, setKey] = useState("");
  const [show, setShow] = useState(false);
  const [status, setStatus] = useState<"valid" | "invalid" | "unknown">("unknown");
  const [copied, setCopied] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    fetchApiKey().then((r: any) => {
      const activeKey = r.key || r.data?.key || "";
      setKey(activeKey);
      setStatus(activeKey ? "valid" : "invalid");
    }).catch(() => setStatus("invalid"));
  }, []);

  function activateLocally(activeKey: string) {
    localStorage.setItem("api_key", activeKey);
    setKey(activeKey);
    setStatus("valid");
  }

  function copy() {
    if (!key) return;
    navigator.clipboard.writeText(key).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1500); });
  }

  async function regen() {
    setSaving(true);
    try {
      const r: any = await regenerateApiKey();
      const generatedKey = r.key || r.data?.key || "";
      if (!generatedKey) throw new Error("Backend did not return the generated admin key");
      // Persist immediately: after regeneration the previous login key is invalid.
      activateLocally(generatedKey);
      setShow(true);
    } finally {
      setSaving(false);
    }
  }

  async function save() {
    if (!key.trim()) return;
    setSaving(true);
    try {
      const r: any = await setApiKey(key.trim());
      const savedKey = r.key || r.data?.key || key.trim();
      activateLocally(savedKey);
      setShow(true);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Card className="border-[var(--border)]">
      <CardHeader>
        <CardTitle className="text-base flex items-center gap-2">
          <span className="text-[var(--primary)]">🔑</span> Admin API Key
        </CardTitle>
        <p className="text-xs text-[var(--muted-foreground)]">
          Used to log in to Luminus dashboard. Public keys below cannot access management APIs.
        </p>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex gap-2">
          <div className="relative flex-1">
            <Input
              type={show ? "text" : "password"}
              value={key}
              onChange={(e) => setKey(e.target.value)}
              className="font-mono pr-10"
            />
            <button
              onClick={() => setShow((v) => !v)}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-[var(--muted-foreground)] hover:text-[var(--foreground)]"
            >
              {show ? <EyeOff size={14} /> : <Eye size={14} />}
            </button>
          </div>
          <Button variant="outline" size="icon" onClick={copy} title="Copy" disabled={!key}>
            {copied ? <Check size={14} className="text-[var(--primary)]" /> : <Copy size={14} />}
          </Button>
        </div>
        <div className="rounded-md border border-[var(--primary)]/25 bg-[var(--primary)]/5 px-3 py-2 text-xs text-[var(--muted-foreground)]">
          Generate immediately replaces the current dashboard login key. The new key is revealed and saved to this browser automatically; copy it before clearing browser data.
        </div>
        <div className="flex items-center gap-3 flex-wrap">
          <span className="text-xs text-[var(--muted-foreground)]">
            Status: <span className={status === "valid" ? "text-[var(--primary)]" : status === "invalid" ? "text-[var(--error)]" : "text-[var(--muted-foreground)]"}>{status}</span>
          </span>
          <Button variant="outline" size="sm" onClick={regen} disabled={saving}><RefreshCw size={13} className="mr-1" />Generate</Button>
          <Button size="sm" onClick={save} disabled={saving || !key.trim()} className="bg-[var(--primary)] text-white hover:bg-blue-600">
            {saving ? "Saving…" : "Save & Activate"}
          </Button>
        </div>
        <div className="rounded-md bg-[var(--secondary)] p-3 text-xs font-mono text-[var(--muted-foreground)]">
          {`curl http://localhost:1930/v1/chat/completions \\\n  -H "Authorization: Bearer ${show ? key : "sk-****"}" \\\n  -H "Content-Type: application/json" \\\n  -d '{"model":"claude-sonnet-4","messages":[{"role":"user","content":"Hello!"}]}'`}
        </div>
      </CardContent>
    </Card>
  );
}

// ── Public key row ───────────────────────────────────────────────────────────
function KeyRow({
  row, allModels, onUpdate, onDelete,
}: {
  row: ApiKeyRow;
  allModels: string[];
  onUpdate: (id: number, patch: Partial<ApiKeyRow>) => void;
  onDelete: (id: number) => void;
}) {
  const [show, setShow] = useState(false);
  const [copied, setCopied] = useState(false);
  const [localModels, setLocalModels] = useState<string[]>(row.allowedModels || []);
  const [limit, setLimit] = useState(String(row.totalTokenLimit || 0));
  const [saving, setSaving] = useState(false);

  function copy() {
    navigator.clipboard.writeText(row.key).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1500); });
  }

  function toggleModel(m: string) {
    setLocalModels((prev) => prev.includes(m) ? prev.filter((x) => x !== m) : [...prev, m]);
  }

  async function save() {
    setSaving(true);
    try {
      await updateCustomKey(row.id, { allowedModels: localModels, totalTokenLimit: Number(limit) || 0 });
      onUpdate(row.id, { allowedModels: localModels, totalTokenLimit: Number(limit) || 0 });
    } finally { setSaving(false); }
  }

  async function toggle() {
    await updateCustomKey(row.id, { enabled: !row.enabled });
    onUpdate(row.id, { enabled: !row.enabled });
  }

  const used = row.totalTokensUsed || 0;
  const lim = row.totalTokenLimit || 0;
  const remaining = lim > 0 ? Math.max(0, lim - used) : null;
  const pct = lim > 0 ? Math.min(100, (used / lim) * 100) : 0;

  return (
    <div className="rounded-lg border border-[var(--border)] bg-[var(--card)] p-4 space-y-4">
      {/* Header row */}
      <div className="flex flex-wrap items-start gap-3 justify-between">
        <div className="space-y-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="text-sm font-semibold text-[var(--foreground)]">{row.name}</span>
            <span className={`text-xs px-2 py-0.5 rounded-full border ${row.enabled ? "border-[var(--primary)] text-[var(--primary)]" : "border-[var(--muted-foreground)] text-[var(--muted-foreground)]"}`}>
              {row.enabled ? "active" : "disabled"}
            </span>
            <span className="text-xs px-2 py-0.5 rounded-full border border-[var(--border)] text-[var(--muted-foreground)]">public-only</span>
          </div>
          <div className="flex items-center gap-2">
            <code className="text-xs font-mono text-[var(--muted-foreground)]">
              {show ? row.key : row.key.slice(0, 10) + "••••••••••••"}
            </code>
            <button onClick={() => setShow((v) => !v)} className="text-[var(--muted-foreground)] hover:text-[var(--foreground)]">
              {show ? <EyeOff size={12} /> : <Eye size={12} />}
            </button>
            <button onClick={copy} className="text-[var(--muted-foreground)] hover:text-[var(--primary)]">
              {copied ? <Check size={12} className="text-[var(--primary)]" /> : <Copy size={12} />}
            </button>
          </div>
        </div>
        <div className="flex gap-2 shrink-0">
          <Button variant="outline" size="sm" onClick={toggle}>
            <Power size={13} className="mr-1" />{row.enabled ? "Disable" : "Enable"}
          </Button>
          <Button variant="outline" size="sm" onClick={() => onDelete(row.id)} className="text-[var(--error)] hover:text-[var(--error)]">
            <Trash2 size={13} />
          </Button>
        </div>
      </div>

      {/* Usage bar */}
      <div className="space-y-1">
        <div className="flex justify-between text-xs text-[var(--muted-foreground)]">
          <span>Used / Limit</span>
          <span>{fmt(used)} / {lim > 0 ? fmt(lim) : "∞"}{remaining !== null ? ` · ${fmt(remaining)} left` : ""} · req {row.totalRequests || 0}</span>
        </div>
        {lim > 0 && (
          <div className="h-1.5 rounded-full bg-[var(--secondary)] overflow-hidden">
            <div className="h-full rounded-full bg-[var(--primary)] transition-all" style={{ width: `${pct}%` }} />
          </div>
        )}
      </div>

      {/* Token limit input */}
      <div className="flex items-center gap-2">
        <span className="text-xs text-[var(--muted-foreground)] shrink-0">Token limit:</span>
        <Input
          type="number"
          value={limit}
          onChange={(e) => setLimit(e.target.value)}
          className="h-7 text-xs w-36"
          placeholder="0 = unlimited"
        />
      </div>

      {/* Model checklist */}
      <div>
        <p className="text-xs text-[var(--muted-foreground)] mb-2">
          Allowed models <span className="text-[var(--primary)]">({localModels.length === 0 ? "all" : localModels.length + " selected"})</span>
        </p>
        <div className="max-h-40 overflow-y-auto grid grid-cols-1 sm:grid-cols-2 gap-1 pr-1">
          {allModels.map((m) => (
            <label key={m} className="flex items-center gap-2 cursor-pointer rounded px-2 py-1 hover:bg-[var(--secondary)] text-xs">
              <input
                type="checkbox"
                checked={localModels.includes(m)}
                onChange={() => toggleModel(m)}
                className="accent-[var(--primary)]"
              />
              <span className="truncate font-mono text-[var(--foreground)]">{m}</span>
            </label>
          ))}
          {allModels.length === 0 && <p className="text-xs text-[var(--muted-foreground)] col-span-2">No models available</p>}
        </div>
      </div>

      <Button size="sm" onClick={save} disabled={saving} className="bg-[var(--primary)] text-white hover:bg-blue-600">
        {saving ? "Saving…" : "Save"}
      </Button>
    </div>
  );
}

// ── Create form ──────────────────────────────────────────────────────────────
function CreateKeyForm({ allModels, onCreate }: { allModels: string[]; onCreate: (row: ApiKeyRow) => void }) {
  const [name, setName] = useState("");
  const [keyVal, setKeyVal] = useState(generateKey());
  const [limit, setLimit] = useState("10000000");
  const [selected, setSelected] = useState<string[]>([]);
  const [expiry, setExpiry] = useState("");
  const [creating, setCreating] = useState(false);

  function toggleModel(m: string) {
    setSelected((prev) => prev.includes(m) ? prev.filter((x) => x !== m) : [...prev, m]);
  }

  async function create() {
    if (!name.trim()) return;
    setCreating(true);
    try {
      const r = await createCustomKey({
        name: name.trim(),
        key: keyVal,
        allowedModels: selected,
        totalTokenLimit: Number(limit) || 0,
        expiresAt: expiry || null,
      }) as any;
      onCreate(r.data);
      setName("");
      setKeyVal(generateKey());
      setSelected([]);
      setLimit("10000000");
      setExpiry("");
    } finally { setCreating(false); }
  }

  return (
    <Card className="border-[var(--border)]">
      <CardHeader>
        <CardTitle className="text-base">Buat API key</CardTitle>
        <p className="text-xs text-[var(--muted-foreground)]">Public-only — hanya bisa akses /v1/* proxy, tidak bisa login Luminus.</p>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex flex-wrap gap-2">
          <Input placeholder="nama" value={name} onChange={(e) => setName(e.target.value)} className="flex-1 min-w-32" />
          <Input type="number" value={limit} onChange={(e) => setLimit(e.target.value)} className="w-36" placeholder="token limit" />
          <Input type="date" value={expiry} onChange={(e) => setExpiry(e.target.value)} className="w-40" title="Expiry date (optional)" />
          <Button onClick={create} disabled={creating || !name.trim()} className="bg-[var(--primary)] text-white hover:bg-blue-600">
            <Plus size={14} className="mr-1" />{creating ? "Creating…" : `Create ${keyVal.slice(0, 10)}…`}
          </Button>
        </div>
        <div>
          <p className="text-xs text-[var(--muted-foreground)] mb-2">Select models <span className="text-[var(--primary)]">({selected.length === 0 ? "all allowed" : selected.length + " selected"})</span></p>
          <div className="max-h-48 overflow-y-auto grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-1 pr-1">
            {allModels.map((m) => (
              <label key={m} className="flex items-center gap-2 cursor-pointer rounded px-2 py-1 hover:bg-[var(--secondary)] text-xs">
                <input type="checkbox" checked={selected.includes(m)} onChange={() => toggleModel(m)} className="accent-[var(--primary)]" />
                <span className="truncate font-mono text-[var(--foreground)]">{m}</span>
              </label>
            ))}
            {allModels.length === 0 && <p className="text-xs text-[var(--muted-foreground)]">Loading models…</p>}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

// ── Page ─────────────────────────────────────────────────────────────────────
export default function ApiKey() {
  const [keys, setKeys] = useState<ApiKeyRow[]>([]);
  const [allModels, setAllModels] = useState<string[]>([]);

  useEffect(() => {
    fetchCustomKeys().then((r) => setKeys(r.data || [])).catch(() => {});
    fetchModels().then((r: any) => {
      setAllModels((r.data || []).map((m: any) => m.id || `${m.provider}/${m.model}`));
    }).catch(() => {});
  }, []);

  function handleCreate(row: ApiKeyRow) {
    setKeys((prev) => [row, ...prev]);
  }

  function handleUpdate(id: number, patch: Partial<ApiKeyRow>) {
    setKeys((prev) => prev.map((k) => k.id === id ? { ...k, ...patch } : k));
  }

  async function handleDelete(id: number) {
    await deleteCustomKey(id);
    setKeys((prev) => prev.filter((k) => k.id !== id));
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-[var(--foreground)]">API Key</h1>
        <p className="text-sm text-[var(--muted-foreground)] mt-1">Manage admin key and public proxy keys</p>
      </div>

      <AdminKeySection />

      <CreateKeyForm allModels={allModels} onCreate={handleCreate} />

      <div className="space-y-4">
        <h2 className="text-base font-semibold text-[var(--foreground)]">API keys <span className="text-[var(--muted-foreground)] font-normal text-sm">({keys.length})</span></h2>
        {keys.length === 0 ? (
          <p className="text-sm text-[var(--muted-foreground)]">No public keys yet. Create one above.</p>
        ) : (
          keys.map((k) => (
            <KeyRow key={k.id} row={k} allModels={allModels} onUpdate={handleUpdate} onDelete={handleDelete} />
          ))
        )}
      </div>
    </div>
  );
}
