import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";

interface UsageChartProps {
  data?: any[];
  period?: string;
  colorsByModel?: Record<string, string>;
}

const defaultData: any[] = [];

function formatTokenCount(value: number) {
  const abs = Math.abs(value);
  const format = (num: number) => Number(num.toFixed(2)).toString();
  if (abs >= 1_000_000) return `${format(value / 1_000_000)}M`;
  if (abs >= 1_000) return `${format(value / 1_000)}K`;
  return value.toString();
}

/**
 * Single-line cumulative token usage chart rendered in Luminus blue.
 * The chart aggregates all per-model tokens into one series so the line
 * always reads as total traffic.
 */
export default function UsageChart({ data = defaultData }: UsageChartProps) {
  if (data.length === 0) {
    return (
      <div className="h-[260px] w-full flex items-center justify-center rounded-md bg-[var(--secondary)] text-sm text-[var(--muted-foreground)]">
        No usage data yet
      </div>
    );
  }

  // Aggregate per-bucket totals so we get a single cumulative series.
  const aggregated = data.map((row) => {
    let total = 0;
    for (const key of Object.keys(row)) {
      if (key === "hour" || key === "label") continue;
      total += Number(row[key] || 0);
    }
    return { label: row.label, total };
  });

  return (
    <div className="h-[260px] w-full">
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={aggregated} margin={{ top: 10, right: 10, left: 0, bottom: 0 }}>
          <defs>
            <linearGradient id="usage-blue-gradient" x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor="#3b82f6" stopOpacity={0.45} />
              <stop offset="95%" stopColor="#3b82f6" stopOpacity={0} />
            </linearGradient>
          </defs>
          <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
          <XAxis
            dataKey="label"
            stroke="var(--muted-foreground)"
            fontSize={11}
            tickLine={false}
            axisLine={false}
          />
          <YAxis
            stroke="var(--muted-foreground)"
            fontSize={11}
            tickLine={false}
            axisLine={false}
            tickFormatter={(value) => formatTokenCount(Number(value))}
          />
          <Tooltip
            content={({ active, payload, label }) => {
              if (!active || !payload?.length) return null;
              return (
                <div
                  style={{
                    backgroundColor: "var(--popover)",
                    border: "1px solid var(--border)",
                    borderRadius: "8px",
                    padding: "8px 12px",
                  }}
                >
                  <p style={{ color: "var(--muted-foreground)", marginBottom: 4, fontSize: 12 }}>
                    {label}
                  </p>
                  <p style={{ color: "#3b82f6", fontSize: 12, margin: "2px 0" }}>
                    Total : {formatTokenCount(Number(payload[0]?.value || 0))}
                  </p>
                </div>
              );
            }}
          />
          <Area
            type="monotone"
            dataKey="total"
            stroke="#3b82f6"
            fill="url(#usage-blue-gradient)"
            strokeWidth={2}
            isAnimationActive={true}
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}
