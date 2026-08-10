import { useEffect, useState } from "react";
import { fetchCostSummary, fetchUsage } from "@/lib/api";
import { useWsEvent } from "@/hooks/useWebSocket";
import UsageChart from "@/components/dashboard/UsageChart";
import { Zap, ArrowDown, ArrowUp, DollarSign } from "lucide-react";

function fmt(n: number): string {
  if (n >= 1000000000) return `${(n / 1000000000).toFixed(2)}B`;
  if (n >= 1000000) return `${(n / 1000000).toFixed(2)}M`;
  if (n >= 1000) return `${(n / 1000).toFixed(1)}K`;
  return String(n);
}

interface StatCard {
  icon: React.ReactNode;
  label: string;
  value: string | number;
  color: string;
}

function StatPill({ icon, label, value, color }: StatCard) {
  return (
    <div className="flex flex-col gap-1 rounded-md bg-[var(--secondary)] border border-[var(--border)] px-4 py-3">
      <div className="flex items-center gap-2 text-[var(--muted-foreground)] text-xs uppercase tracking-wide">
        <span style={{ color }}>{icon}</span>
        {label}
      </div>
      <p className="text-lg font-bold text-[var(--foreground)]">{value}</p>
    </div>
  );
}

export default function UsageNew() {
  const [costData, setCostData] = useState<any>(null);
  const [chartRows, setChartRows] = useState<any[]>([]);

  async function load() {
    await Promise.all([
      fetchCostSummary(48).then(setCostData).catch(() => setCostData(null)),
      fetchUsage(48).then((r: any) => setChartRows(r.data || [])).catch(() => setChartRows([])),
    ]);
  }

  useEffect(() => { load(); }, []);
  useWsEvent(["request_log"], load);

  const totals = costData?.totals || {};
  const totalRequests = Number(totals.requests || 0);
  const inputTokens = Number(totals.inputTokens || 0);
  const outputTokens = Number(totals.outputTokens || 0);
  const estCost = Number(totals.totalCost || 0);

  return (
    <div className="min-h-screen bg-[var(--background)] p-6">
      <div className="max-w-[1600px] mx-auto space-y-6">
        <div className="flex items-center gap-3">
          <Zap className="text-[#3b82f6]" size={28} />
          <div>
            <h1 className="text-2xl font-bold">Usage & Analytics</h1>
            <p className="text-sm text-[var(--muted-foreground)]">Monitor API usage, token consumption, and request logs</p>
          </div>
        </div>
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-5 gap-4">
          <StatPill icon={<Zap size={16} />} label="Total Requests" value={fmt(totalRequests)} color="var(--foreground)" />
          <StatPill icon={<ArrowUp size={16} />} label="Input Tokens" value={fmt(inputTokens)} color="#f97316" />
          <StatPill icon={<ArrowDown size={16} />} label="Cached Tokens" value="0" color="#06b6d4" />
          <StatPill icon={<ArrowUp size={16} />} label="Output Tokens" value={fmt(outputTokens)} color="#10b981" />
          <StatPill icon={<DollarSign size={16} />} label="Est. Cost" value={`~$${estCost.toFixed(2)}`} color="#eab308" />
        </div>
        <div className="rounded-lg border border-[var(--border)] bg-[var(--secondary)] p-6">
          <h3 className="text-sm font-semibold mb-4">Token Usage (48h)</h3>
          <UsageChart data={chartRows} />
        </div>
        <div className="rounded-lg border border-[var(--border)] bg-[var(--secondary)] p-6">
          <h3 className="text-sm font-semibold mb-4">Usage by Model</h3>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead className="text-xs text-[var(--muted-foreground)] uppercase border-b border-[var(--border)]">
                <tr><th className="text-left p-3">Model</th><th className="text-left p-3">Provider</th><th className="text-right p-3">Requests</th><th className="text-right p-3">Input Cost</th><th className="text-right p-3">Output Cost</th><th className="text-right p-3">Total Cost</th></tr>
              </thead>
              <tbody>
                {(costData?.data || []).map((row: any, i: number) => (
                  <tr key={i} className="border-b border-[var(--border)] hover:bg-[var(--accent)]">
                    <td className="p-3 font-mono text-sm">{row.model}</td>
                    <td className="p-3 text-[var(--muted-foreground)]">{row.provider || "—"}</td>
                    <td className="text-right p-3">{row.requests}</td>
                    <td className="text-right p-3">${row.inputCost.toFixed(2)}</td>
                    <td className="text-right p-3">${row.outputCost.toFixed(2)}</td>
                    <td className="text-right p-3 font-semibold text-[#eab308]">${row.totalCost.toFixed(2)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
}
