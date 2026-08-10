import { useEffect, useRef, useState } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Activity, Database, AlertTriangle, Gauge, Cpu, MemoryStick, RefreshCw } from "lucide-react";
import { useWsEvent } from "@/hooks/useWebSocket";
import { fetchApi } from "@/lib/api";

interface HealthResponse {
  status: string;
  uptime: number;
  timestamp: string;
  memory: {
    rss: number;
    heapUsed: number;
    heapTotal: number;
    external: number;
    arrayBuffers: number;
    bunRuntime: number;
  } | null;
  pid: number | null;
  cpu: { user: number; system: number } | null;
  system: { cores: number; totalmem: number };
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "0 MB";
  const mb = bytes / (1024 * 1024);
  if (mb < 1024) return `${mb.toFixed(0)} MB`;
  const gb = mb / 1024;
  return `${gb.toFixed(1)} GB`;
}

export default function Health({ stats }: { stats: any }) {
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cpuPercent, setCpuPercent] = useState(0);
  const previousCpuRef = useRef<{ total: number; at: number } | null>(null);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchApi<HealthResponse>("/api/health");
      setHealth(data);

      if (data?.cpu && data?.system?.cores) {
        const currentTotal = data.cpu.user + data.cpu.system;
        const now = Date.now();
        if (previousCpuRef.current) {
          const deltaCpu = currentTotal - previousCpuRef.current.total;
          const deltaMs = now - previousCpuRef.current.at;
          const deltaUs = deltaMs * 1000;

          let pct = (deltaCpu / deltaUs) * 100;
          pct = pct / data.system.cores;
          setCpuPercent(Math.max(0, Math.min(pct, 100)));
        }
        previousCpuRef.current = { total: currentTotal, at: now };
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load health");
    } finally {
      setLoading(false);
    }
  }

  // Refresh health every 15 seconds while mounted.
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  useEffect(() => {
    load();
    intervalRef.current = setInterval(load, 15000);
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, []);

  useWsEvent(
    ["request_log", "request_error", "account_status", "account_updated", "account_created", "account_deleted"],
    load,
  );

  const totalRequests = Number(stats?.requests?.total || 0);
  const errorRequests = Number(stats?.requests?.errors || 0);
  const successRequests = Number(stats?.requests?.success || 0);
  const errorRate = totalRequests > 0 ? (errorRequests / totalRequests) * 100 : 0;
  const cacheRate = 0; // not tracked yet; reserved for cache stats in future
  const latencyMs = Number(stats?.performance?.avgDurationMs || 0);
  const providerCount = Number(stats?.pool?.total || 0);

  const metrics = [
    {
      label: "Latency",
      value: `${latencyMs}ms`,
      sub: "Avg duration",
      icon: Activity,
      color: "var(--primary)",
    },
    {
      label: "Cache",
      value: `${cacheRate}%`,
      sub: "Cache rate",
      icon: Database,
      color: "var(--info)",
    },
    {
      label: "Errors",
      value: `${errorRate.toFixed(1)}%`,
      sub: "Error rate",
      icon: AlertTriangle,
      color: "var(--error)",
    },
    {
      label: "Registry",
      value: providerCount.toString(),
      sub: "Providers",
      icon: Gauge,
      color: "var(--success)",
    },
  ];

  const mem = health?.memory;
  const rssMB = mem ? mem.rss / (1024 * 1024) : 0;
  const heapMB = mem ? mem.heapUsed / (1024 * 1024) : 0;
  const bunMB = mem ? mem.bunRuntime / (1024 * 1024) : 0;
  const extMB = mem ? mem.external / (1024 * 1024) : 0;
  const arrMB = mem ? mem.arrayBuffers / (1024 * 1024) : 0;
  const systemTotal = health?.system?.totalmem ?? 0;
  const systemTotalStr = formatBytes(systemTotal);
  const rssPct = systemTotal > 0 ? (mem?.rss ?? 0) / systemTotal * 100 : 0;

  const memBars = [
    { label: "JS heap", value: heapMB, color: "var(--primary)" },
    { label: "Bun runtime", value: bunMB, color: "var(--success)" },
    { label: "External", value: extMB, color: "var(--warning)" },
    { label: "Array buffers", value: arrMB, color: "var(--info)" },
  ];
  const maxBar = Math.max(...memBars.map((b) => b.value), 1);

  // CPU usage: percentage computed by sampling process.cpuUsage across two
  // health snapshots (delta microseconds / elapsed microseconds, divided
  // by the number of logical cores to get a per-core average).
  const cpuPct = cpuPercent;
  const cpuUserSec = health?.cpu ? health.cpu.user / 1_000_000 : 0;
  const cpuSysSec = health?.cpu ? health.cpu.system / 1_000_000 : 0;
  const cores = health?.system?.cores ?? 0;
  const pid = health?.pid ?? null;

  return (
    <div className="space-y-4">
      <div className="flex flex-col gap-1.5 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-center gap-2">
          <div className="w-7 h-7 rounded-md bg-[var(--success)]/15 flex items-center justify-center">
            <Gauge className="w-4 h-4 text-[var(--success)]" />
          </div>
          <div>
            <h2 className="text-base font-semibold text-[var(--foreground)] leading-none">Health</h2>
            <p className="text-xs text-[var(--muted-foreground)] mt-1">
              Last 24 hours · runtime resource usage
            </p>
          </div>
        </div>
        <button
          onClick={load}
          disabled={loading}
          className="text-xs text-[var(--muted-foreground)] hover:text-[var(--foreground)] transition-colors flex items-center gap-1"
          title="Refresh health snapshot"
        >
          <RefreshCw className={`w-3 h-3 ${loading ? "animate-spin" : ""}`} />
          {loading ? "Refreshing..." : "Refresh"}
        </button>
      </div>

      {/* Metric cards row */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        {metrics.map((m) => (
          <Card key={m.label} className="border-[var(--border)] bg-[var(--card)]">
            <CardContent className="p-4">
              <div className="flex items-center justify-between mb-2">
                <div className="w-7 h-7 rounded-md flex items-center justify-center" style={{ backgroundColor: `color-mix(in srgb, ${m.color} 15%, transparent)` }}>
                  <m.icon className="w-4 h-4" style={{ color: m.color }} />
                </div>
              </div>
              <div className="text-xl font-bold text-[var(--foreground)] leading-none">{m.value}</div>
              <div className="text-xs text-[var(--muted-foreground)] mt-1.5">{m.sub}</div>
            </CardContent>
          </Card>
        ))}
      </div>

      {/* RAM + CPU usage row */}
      <div className="grid gap-3 md:grid-cols-2">
        {/* RAM usage */}
        <Card className="border-[var(--border)] bg-[var(--card)]">
          <CardContent className="p-4 space-y-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <MemoryStick className="w-4 h-4 text-[var(--primary)]" />
                <div>
                  <div className="text-sm font-semibold text-[var(--foreground)] leading-none">RAM usage</div>
                  <div className="text-xs text-[var(--muted-foreground)] mt-1">Bun Runtime · Luminus process</div>
                </div>
              </div>
              <span className="text-[10px] rounded-full bg-[var(--primary)]/15 text-[var(--primary)] px-2 py-0.5 font-medium">
                {systemTotalStr} system
              </span>
            </div>

            <div className="flex items-baseline gap-2">
              <span className="text-2xl font-bold text-[var(--foreground)]">{rssMB.toFixed(0)} MB</span>
              <span className="text-xs text-[var(--muted-foreground)]">RSS</span>
            </div>

            <div className="h-2 rounded-full bg-[var(--secondary)] overflow-hidden">
              <div
                className="h-full bg-[var(--primary)] rounded-full transition-all"
                style={{ width: `${Math.min(100, Math.max(0.3, rssPct))}%` }}
              />
            </div>

            <p className="text-[11px] text-[var(--muted-foreground)] leading-relaxed">
              RSS is the whole Luminus process. The larger portion is Bun runtime/native/JIT overhead; the JS heap is only one part of it.
            </p>

            <div className="grid grid-cols-2 gap-2 pt-1">
              {memBars.map((b) => (
                <div key={b.label} className="rounded-md border border-[var(--border)] bg-[var(--secondary)]/30 p-2">
                  <div className="flex items-center justify-between text-[11px] mb-1.5">
                    <span className="text-[var(--muted-foreground)]">{b.label}</span>
                    <span className="font-medium text-[var(--foreground)]">{b.value.toFixed(0)} MB</span>
                  </div>
                  <div className="h-1.5 rounded-full bg-[var(--background)] overflow-hidden">
                    <div
                      className="h-full rounded-full"
                      style={{
                        width: `${(b.value / maxBar) * 100}%`,
                        backgroundColor: b.color,
                      }}
                    />
                  </div>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>

        {/* CPU usage */}
        <Card className="border-[var(--border)] bg-[var(--card)]">
          <CardContent className="p-4 space-y-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Cpu className="w-4 h-4 text-[var(--warning)]" />
                <div>
                  <div className="text-sm font-semibold text-[var(--foreground)] leading-none">CPU usage</div>
                  <div className="text-xs text-[var(--muted-foreground)] mt-1">Current process load</div>
                </div>
              </div>
              <span className="text-[10px] rounded-full bg-[var(--success)]/15 text-[var(--success)] px-2 py-0.5 font-medium flex items-center gap-1">
                <span className="inline-block w-1.5 h-1.5 rounded-full bg-[var(--success)]" />
                Live
              </span>
            </div>

            <div className="flex items-center gap-4">
              {/* Circular progress ring */}
              <div className="relative w-24 h-24 flex-shrink-0">
                <svg className="w-full h-full -rotate-90" viewBox="0 0 100 100">
                  <circle
                    cx="50"
                    cy="50"
                    r="42"
                    fill="none"
                    stroke="var(--secondary)"
                    strokeWidth="6"
                  />
                  <circle
                    cx="50"
                    cy="50"
                    r="42"
                    fill="none"
                    stroke="var(--warning)"
                    strokeWidth="6"
                    strokeLinecap="round"
                    strokeDasharray={`${2 * Math.PI * 42}`}
                    strokeDashoffset={`${2 * Math.PI * 42 * (1 - cpuPct / 100)}`}
                    className="transition-all duration-300"
                  />
                </svg>
                <div className="absolute inset-0 flex items-center justify-center">
                  <span className="text-lg font-bold text-[var(--foreground)]">{cpuPct.toFixed(1)}%</span>
                </div>
              </div>

              <div className="flex-1 space-y-2 text-sm">
                <div className="flex items-center justify-between">
                  <span className="text-[var(--muted-foreground)] text-xs">Cores</span>
                  <span className="rounded-md bg-[var(--secondary)] px-2 py-0.5 text-xs font-mono text-[var(--foreground)]">
                    {cores} logical
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-[var(--muted-foreground)] text-xs">PID</span>
                  <span className="rounded-md bg-[var(--secondary)] px-2 py-0.5 text-xs font-mono text-[var(--foreground)]">
                    {pid ?? "—"}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-[var(--muted-foreground)] text-xs">User</span>
                  <span className="text-xs font-mono text-[var(--foreground)]">{cpuUserSec.toFixed(1)}s</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-[var(--muted-foreground)] text-xs">Sys</span>
                  <span className="text-xs font-mono text-[var(--foreground)]">{cpuSysSec.toFixed(1)}s</span>
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>

      {error && (
        <div className="rounded-md border border-[var(--error)]/30 bg-[var(--error)]/10 p-3 text-xs text-[var(--error)]">
          {error}
        </div>
      )}
    </div>
  );
}
