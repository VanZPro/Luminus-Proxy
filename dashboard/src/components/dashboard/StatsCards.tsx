import { Users, Activity, CheckCircle, Zap } from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";

function formatTokens(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}

interface StatsData {
  accounts: { active: number; total: number };
  requests: number;
  successRate: number;
  totalTokens: number;
}

interface StatsCardsProps {
  data?: StatsData;
}

const defaultData: StatsData = {
  accounts: { active: 0, total: 0 },
  requests: 0,
  successRate: 0,
  totalTokens: 0,
};

export default function StatsCards({ data = defaultData }: StatsCardsProps) {
  const stats = [
    {
      label: "Accounts",
      value: `${data.accounts.active}/${data.accounts.total}`,
      subtitle: `active`,
      icon: Users,
    },
    {
      label: "Requests",
      value: data.requests.toLocaleString(),
      subtitle: "All time",
      icon: Activity,
    },
    {
      label: "Success Rate",
      value: `${data.successRate}%`,
      subtitle: "All time",
      icon: CheckCircle,
    },
    {
      label: "Total Tokens",
      value: formatTokens(data.totalTokens),
      subtitle: "All time",
      icon: Zap,
    },
  ];

  return (
    <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
      {stats.map((stat) => (
        <Card
          key={stat.label}
          className="transition-colors hover:border-[var(--muted-foreground)]"
        >
          <CardContent className="p-4">
            <div className="flex items-center justify-between">
              <div className="min-w-0">
                <p className="text-xs text-[var(--muted-foreground)] uppercase tracking-wide">
                  {stat.label}
                </p>
                <p className="text-2xl font-bold mt-1 text-[var(--foreground)] tabular-nums">
                  {stat.value}
                </p>
                <p className="text-xs text-[var(--muted-foreground)] mt-1">
                  {stat.subtitle}
                </p>
              </div>
              <div className="p-2.5 rounded-md bg-[var(--secondary)] border border-[var(--border)]">
                <stat.icon className="w-4 h-4 text-[var(--muted-foreground)]" />
              </div>
            </div>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}
