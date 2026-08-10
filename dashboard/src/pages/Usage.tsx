import { useEffect, useRef, useState } from "react";
import { fetchAccounts, fetchCostSummary, fetchUsage, fetchRequests } from "@/lib/api";
import { useWsEvent } from "@/hooks/useWebSocket";
import UsageChart from "@/components/dashboard/UsageChart";
import { Zap, ArrowDown, ArrowUp, DollarSign, Radio, Activity } from "lucide-react";

function fmt(n: number) { if (n >= 1e9) return `${(n / 1e9).toFixed(2)}B`; if (n >= 1e6) return `${(n / 1e6).toFixed(2)}M`; if (n >= 1e3) return `${(n / 1e3).toFixed(1)}K`; return n.toLocaleString(); }
function ago(value: string) { const ms = Date.now() - new Date(value).getTime(); if (!Number.isFinite(ms)) return "—"; const s = Math.max(0, Math.floor(ms / 1000)); return s < 60 ? `${s}s ago` : s < 3600 ? `${Math.floor(s / 60)}m ago` : `${Math.floor(s / 3600)}h ago`; }

function Stat({ icon, label, value, color }: { icon: React.ReactNode; label: string; value: string; color: string }) {
  return <div className="rounded-lg border border-[var(--border)] bg-[var(--card)] px-4 py-3"><div className="flex items-center gap-2 text-xs uppercase tracking-wide text-[var(--muted-foreground)]"><span style={{ color }}>{icon}</span>{label}</div><div className="mt-1 text-xl font-bold text-[var(--foreground)]">{value}</div></div>;
}

const CONNECTED_PROVIDER_ORDER = ["codebuddy", "carthe", "claudeshen", "gptshen", "risu", "zen"];
const PROVIDER_ALIASES: Record<string, string> = {
  "custom:carthe": "carthe",
  "custom_carthe": "carthe",
  "custom-carth": "carthe",
  "custom:claudeshen": "claudeshen",
  "custom_claudeshen": "claudeshen",
  "custom:gptshen": "gptshen",
  "custom_gptshen": "gptshen",
  "custom:risu": "risu",
  "custom_risu": "risu",
  "custom:zen": "zen",
  "custom_zen": "zen",
  "code-buddy": "codebuddy",
};
function providerKey(value: unknown) {
  const raw = String(value || "").trim().toLowerCase();
  return PROVIDER_ALIASES[raw] || raw.replace(/^custom[-_:]/, "").split(/[/:]/)[0];
}

function resolveEventProvider(event: any): string | null {
  const provider = event?.provider || event?.data?.provider || event?.data?.accountProvider || event?.account?.provider;
  const model = event?.model || event?.data?.model;
  if (String(provider).toLowerCase() === "byok") {
    const hit = ["carthe", "claudeshen", "gptshen", "risu", "zen"].find(name => String(model).toLowerCase().startsWith(name));
    return hit || null;
  }
  return provider ? String(provider) : null;
}

function RecentRequests({ requests }: { requests: any[] }) {
  return <div className="usage-recent-panel rounded-lg border border-[var(--border)] bg-[var(--card)] p-5">
    <h3 className="mb-4 text-xs font-semibold uppercase tracking-wider text-[var(--muted-foreground)]">Recent Requests</h3>
    <div className="usage-recent-scroll">
      <table className="w-full text-xs"><thead className="sticky top-0 bg-[var(--card)] text-left text-[10px] uppercase tracking-wide text-[var(--muted-foreground)]"><tr><th className="pb-3">Model</th><th className="pb-3">In / Out</th><th className="pb-3 text-right">When</th></tr></thead>
      <tbody>{requests.map((r, i) => <tr key={r.id || i} className="border-t border-[var(--border)]/70"><td className="py-3 pr-2"><span className={`mr-2 inline-block h-1.5 w-1.5 rounded-full ${r.status === "error" ? "bg-[var(--error)]" : "bg-[var(--success)]"}`} />{r.model || "unknown"}</td><td className="whitespace-nowrap py-3"><span className="text-[#fb7185]">{fmt(Number(r.promptTokens || r.inputTokens || 0))}↑</span> <span className="text-[#4ade80]">{fmt(Number(r.completionTokens || r.outputTokens || 0))}↓</span></td><td className="whitespace-nowrap py-3 text-right text-[var(--muted-foreground)]">{ago(r.createdAt || r.created_at)}</td></tr>)}</tbody></table>
    </div>
  </div>;
}

function ProviderNetwork({ accounts, activeProvider }: { accounts: any[]; activeProvider: string | null }) {
  const connected = new Set(accounts.filter(a => a?.enabled !== false && a?.status !== "disabled")
    .map(a => providerKey(a?.provider === "byok" ? String(a?.email || "").split("#")[0] : (a?.provider || a?.type || a?.name)))
    .filter(name => CONNECTED_PROVIDER_ORDER.includes(name)));
  const providers = CONNECTED_PROVIDER_ORDER.filter(name => connected.has(name));
  const effective = activeProvider ? providerKey(activeProvider) : null;
  const [zoom, setZoom] = useState(1);
  const W = 200, H = 100, cx = 100, cy = 50;
  const dots: Record<string, string> = { codebuddy: "#60a5fa", carthe: "#f472b6", claudeshen: "#a78bfa", gptshen: "#fbbf24", risu: "#34d399", zen: "#22d3ee" };
  const nodes = providers.map((name, i) => {
    const angle = (i / providers.length) * Math.PI * 2 - Math.PI / 2;
    const x = cx + Math.cos(angle) * 78;
    const y = cy + Math.sin(angle) * 36;
    const mx = (cx + x) / 2, my = (cy + y) / 2;
    const nx = -(y - cy), ny = (x - cx);
    const len = Math.hypot(nx, ny) || 1;
    const k = (i % 2 === 0 ? 7 : -7);
    const qx = mx + (nx / len) * k, qy = my + (ny / len) * k;
    const d = `M ${cx} ${cy} Q ${qx} ${qy} ${x} ${y}`;
    const hot = effective === name;
    return { name, x, y, d, hot };
  });
  return <div className="relative overflow-hidden rounded-lg border border-[var(--border)] bg-[var(--card)]">
    <div className="flex items-center justify-between border-b border-[var(--border)] px-5 py-4">
      <div><h3 className="text-sm font-semibold">Connected Provider Network</h3><p className="text-xs text-[var(--muted-foreground)]">Only providers with an active account are shown.</p></div>
      <span className="flex items-center gap-1 text-xs text-[var(--success)]"><Radio size={13}/> {providers.length} connected</span>
    </div>
    <div className="net-canvas relative mx-auto h-[300px] w-full overflow-hidden">
      <div className="absolute inset-0 transition-transform duration-300" style={{ transform: `scale(${zoom})` }}>
      <svg className="pointer-events-none absolute inset-0 h-full w-full" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden="true">
        <defs>
          <filter id="wireGlow" x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="1.2" result="blur"/>
            <feMerge><feMergeNode in="blur"/><feMergeNode in="SourceGraphic"/></feMerge>
          </filter>
        </defs>
        {nodes.map(n => (
          <g key={`wire-${n.name}`}>
            <path d={n.d} fill="none" stroke={n.hot ? "#fbbf24" : "#22d3ee"} strokeWidth={n.hot ? 0.9 : 0.35} strokeDasharray={n.hot ? "3 2.2" : "1.6 3"} opacity={n.hot ? 1 : 0.7} className={n.hot ? "net-wire-hot" : "net-wire-idle"} filter={n.hot ? "url(#wireGlow)" : undefined}/>
            {n.hot && [0, 0.45, 0.9].map(delay => (
              <circle key={delay} r="1.5" fill="#fde68a" className="net-packet">
                <animateMotion dur="1.15s" begin={`${delay}s`} repeatCount="indefinite" path={n.d}/>
              </circle>
            ))}
          </g>
        ))}
      </svg>
      <div className="net-luminus absolute left-1/2 top-1/2 z-10 flex h-24 w-24 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full border-2 border-[#fbbf24]/70 bg-[var(--background)] text-center text-sm font-bold text-[var(--foreground)]">Luminus</div>
      {nodes.map(n => (
        <div key={n.name} className="absolute z-10" style={{ left: `${(n.x / W) * 100}%`, top: `${(n.y / H) * 100}%`, transform: "translate(-50%, -50%)" }}>
          <div className={`flex items-center gap-1.5 whitespace-nowrap rounded-full border px-3 py-1.5 text-xs transition-all duration-300 ${n.hot ? "border-[#fbbf24] bg-[#fbbf24]/10 text-[#fbbf24] provider-node-hot" : "border-[var(--border)] bg-[var(--secondary)] text-[var(--foreground)]"}`}>
            <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ backgroundColor: dots[n.name] || "var(--muted-foreground)" }} />
            {n.name}
          </div>
        </div>
      ))}
      </div>
      <div className="absolute bottom-3 left-3 z-20 flex flex-col gap-1 rounded-lg border border-[var(--border)] bg-[var(--card)]/95 p-1 shadow-lg">
        <button onClick={() => setZoom(z => Math.min(1.8, +(z + 0.2).toFixed(2)))} className="flex h-7 w-7 items-center justify-center rounded-md text-sm text-[var(--muted-foreground)] hover:bg-[var(--accent)] hover:text-[var(--foreground)]" title="Zoom in">+</button>
        <button onClick={() => setZoom(z => Math.max(0.6, +(z - 0.2).toFixed(2)))} className="flex h-7 w-7 items-center justify-center rounded-md text-sm text-[var(--muted-foreground)] hover:bg-[var(--accent)] hover:text-[var(--foreground)]" title="Zoom out">−</button>
        <button onClick={() => setZoom(1)} className="flex h-7 w-7 items-center justify-center rounded-md text-[10px] text-[var(--muted-foreground)] hover:bg-[var(--accent)] hover:text-[var(--foreground)]" title="Fit to view">⛶</button>
      </div>
    </div>
  </div>;
}

const USAGE_RANGES = [{ key: "today", label: "Today", hours: 24 }, { key: "24h", label: "24h", hours: 24 }, { key: "7d", label: "7D", hours: 168 }, { key: "30d", label: "30D", hours: 720 }, { key: "60d", label: "60D", hours: 1440 }];

export default function Usage() {
  const [range, setRange] = useState("24h"); const [cost, setCost] = useState<any>(null); const [rows, setRows] = useState<any[]>([]); const [recent, setRecent] = useState<any[]>([]); const [accounts, setAccounts] = useState<any[]>([]); const [activeProvider, setActiveProvider] = useState<string | null>(null); const [loadError, setLoadError] = useState<string | null>(null); const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const load = async () => {
    setLoadError(null);
    const rangeConfig = USAGE_RANGES.find(r => r.key === range) || USAGE_RANGES[1];
    const hours = range === "today" ? 24 : rangeConfig.hours;
    const results = await Promise.allSettled([fetchCostSummary(hours, range), fetchUsage(hours, range), fetchAccounts(), fetchRequests(1, 30)]);
        const [costResult, usageResult, accountsResult, recentResult] = results;
    if (costResult.status === "fulfilled") setCost(costResult.value); else setCost(null);
    if (usageResult.status === "fulfilled") setRows((usageResult.value as any).data || []); else setRows([]);
    if (accountsResult.status === "fulfilled") { const value: any = accountsResult.value; setAccounts(value.data || value.accounts || []); } else setAccounts([]);
    if (recentResult.status === "fulfilled") { const value: any = recentResult.value; setRecent(value.data || value.requests || []); } else setRecent([]);
    const failed = results.find((result) => result.status === "rejected") as PromiseRejectedResult | undefined;
    if (failed) setLoadError(failed.reason instanceof Error ? failed.reason.message : "Unable to load usage data");
  };
  useEffect(() => { load(); return () => { if (timer.current) clearTimeout(timer.current); }; }, [range]);
  useWsEvent(["request_log"], (event: any) => { setActiveProvider(resolveEventProvider(event)); if (timer.current) clearTimeout(timer.current); timer.current = setTimeout(() => setActiveProvider(null), 2500); load(); });
  const t = cost?.totals || {}; const models = cost?.data || []; const chart = rows.map(r => ({ label: r.label || r.hour, total: Number(r.tokens || 0) }));
  const rangeLabel = (USAGE_RANGES.find(r => r.key === range) || USAGE_RANGES[1]).label;
  return <div className="space-y-6">
    <div className="flex flex-wrap items-end justify-between gap-3">
      <div><h1 className="flex items-center gap-2 text-2xl font-bold"><Activity className="text-[var(--primary)]"/>Usage & Analytics</h1><p className="mt-1 text-sm text-[var(--muted-foreground)]">Monitor API usage, token consumption, provider activity, and estimated cost</p></div>
      <div className="flex items-center gap-1 rounded-lg border border-[var(--border)] bg-[var(--card)] p-1">{USAGE_RANGES.map(r => <button key={r.key} onClick={() => setRange(r.key)} className={`rounded-md px-3 py-1 text-xs font-medium transition-colors ${range === r.key ? "bg-[var(--primary)] text-white" : "text-[var(--muted-foreground)] hover:text-[var(--foreground)]"}`}>{r.label}</button>)}</div>
    </div>
    {loadError && <div className="rounded-lg border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-4 py-3 text-sm text-[var(--warning)]">Usage data gagal dimuat: {loadError}. Dashboard membutuhkan Admin API Key, bukan public-only key.</div>}
    <div className="grid grid-cols-2 gap-3 lg:grid-cols-5"><Stat icon={<Zap size={15}/>} label="Total Requests" value={fmt(Number(t.requests || 0))} color="var(--foreground)"/><Stat icon={<ArrowUp size={15}/>} label="Total Input Tokens" value={fmt(Number(t.inputTokens || 0))} color="#f97316"/><Stat icon={<ArrowDown size={15}/>} label="Cached Tokens" value="0" color="#06b6d4"/><Stat icon={<ArrowUp size={15}/>} label="Output Tokens" value={fmt(Number(t.outputTokens || 0))} color="#10b981"/><Stat icon={<DollarSign size={15}/>} label="Est. Cost" value={`~$${Number(t.totalCost || 0).toFixed(2)}`} color="#eab308"/></div>
    <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]"><ProviderNetwork accounts={accounts} activeProvider={activeProvider}/><RecentRequests requests={recent}/></div>
    <div className="rounded-lg border border-[var(--border)] bg-[var(--card)] p-5"><div className="mb-4"><h3 className="text-sm font-semibold">Luminus Traffic</h3><p className="text-xs text-[var(--muted-foreground)]">Token activity over {rangeLabel}</p></div><UsageChart data={chart}/></div>
    <div className="rounded-lg border border-[var(--border)] bg-[var(--card)] p-5"><div className="mb-4 flex items-center justify-between"><div><h3 className="text-sm font-semibold">Usage by Model</h3><p className="text-xs text-[var(--muted-foreground)]">Estimated costs, not provider billing · {rangeLabel}</p></div><span className="text-xs text-[var(--muted-foreground)]">{models.length} models</span></div><div className="overflow-x-auto"><table className="w-full text-sm"><thead className="border-b border-[var(--border)] text-left text-xs uppercase text-[var(--muted-foreground)]"><tr><th className="p-3">Model</th><th className="p-3">Provider</th><th className="p-3 text-right">Requests</th><th className="p-3 text-right">Last used</th><th className="p-3 text-right">Input cost</th><th className="p-3 text-right">Output cost</th><th className="p-3 text-right">Total cost</th></tr></thead><tbody>{models.map((r: any, i: number) => <tr key={`${r.provider}-${r.model}-${i}`} className="border-b border-[var(--border)] hover:bg-[var(--accent)]"><td className="p-3 font-mono">{r.model}</td><td className="p-3 text-[var(--muted-foreground)]">{r.provider || "—"}</td><td className="p-3 text-right">{Number(r.requests || 0).toLocaleString()}</td><td className="p-3 text-right text-[var(--muted-foreground)]">{r.lastUsed ? ago(r.lastUsed) : "—"}</td><td className="p-3 text-right">${Number(r.inputCost || 0).toFixed(2)}</td><td className="p-3 text-right">${Number(r.outputCost || 0).toFixed(2)}</td><td className="p-3 text-right font-semibold text-[#eab308]">${Number(r.totalCost || 0).toFixed(2)}</td></tr>)}</tbody></table></div></div>
  </div>;
}
