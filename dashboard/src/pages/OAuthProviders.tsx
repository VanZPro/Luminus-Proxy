import { useEffect, useState } from "react";
import { RefreshCw, KeyRound, CheckCircle2, XCircle, CircleAlert, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { enableOAuthProvider, disableOAuthProvider, fetchOAuthProviders, type OAuthProviderStatus } from "@/lib/api";

const DEFAULT_PATH = "C:\\Users\\Asus\\AppData\\Roaming\\9router\\db\\data.sqlite";

export default function OAuthProviders() {
  const [path, setPath] = useState(DEFAULT_PATH);
  const [providers, setProviders] = useState<OAuthProviderStatus[]>([]);
  const [loading, setLoading] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function loadProviders() {
    setLoading(true);
    setError(null);
    try {
      const res = await fetchOAuthProviders();
      setProviders(res.providers || []);
    } catch (err) {
      setProviders([]);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  async function toggleProvider(provider: OAuthProviderStatus) {
    setBusyId(provider.id);
    setMessage(null);
    setError(null);
    try {
      if (provider.connected) {
        await disableOAuthProvider(provider.id);
        setMessage(`${provider.name} disconnected.`);
      } else {
        await enableOAuthProvider(provider.id);
        setMessage(`${provider.name} connected.`);
      }
      await loadProviders();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusyId(null);
    }
  }

  useEffect(() => { loadProviders(); }, []);

  const connectedCount = providers.filter((p) => p.connected).length;

  return (
    <div className="space-y-6">
      <div className="rounded-lg border border-[var(--border)] bg-[var(--card)] p-5 shadow-[var(--shadow-card)]">
        <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
          <div>
            <p className="text-xs font-medium uppercase tracking-wider text-[var(--muted-foreground)]">Luminus Auth</p>
            <h1 className="mt-2 text-3xl font-bold text-[var(--foreground)]">OAuth Providers</h1>
            <p className="mt-2 text-sm text-[var(--muted-foreground)]">Sync and manage OAuth provider status using 9router configuration.</p>
          </div>
          <Button variant="outline" onClick={loadProviders} disabled={loading}>
            <RefreshCw className={`mr-2 h-4 w-4 ${loading ? "animate-spin" : ""}`} /> Refresh
          </Button>
        </div>
      </div>

      {(message || error) && (
        <div className={`rounded-md border p-3 text-sm ${message ? "border-[var(--info)]/30 bg-[var(--info)]/10 text-[var(--info)]" : "border-[var(--error)]/30 bg-[var(--error)]/10 text-[var(--error)]"}`}>
          {message || error}
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2"><KeyRound className="h-5 w-5" /> 9router database path</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <Input value={path} onChange={(e) => setPath(e.target.value)} className="font-mono text-xs" />
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
            <div className="rounded-md border border-[var(--border)] bg-[var(--secondary)] p-3">
                          <div className="text-xl font-bold">{providers.length}</div>
                          <div className="text-sm text-[var(--muted-foreground)]">Available providers</div>
            </div>
            <div className="rounded-md border border-[var(--border)] bg-[var(--secondary)] p-3">
                          <div className="text-xl font-bold text-[var(--success)]">{connectedCount}</div>
                          <div className="text-sm text-[var(--muted-foreground)]">Connected</div>
            </div>
          </div>
        </CardContent>
      </Card>

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5">
        {providers.map((provider) => (
          <a
            key={provider.id}
            href={`#`}
            onClick={(e) => {
              e.preventDefault();
              toggleProvider(provider);
            }}
            className="block"
          >
            <Card className="h-full transition-colors hover:border-[var(--muted-foreground)]">
              <CardHeader className="pb-3">
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <CardTitle className="flex items-center gap-2">
                      <img src={`/providers/${provider.icon}`} alt={provider.name} className="h-5 w-5 rounded-full" />
                      {provider.name}
                    </CardTitle>
                    <p className="mt-1 text-xs text-[var(--muted-foreground)]">{provider.description}</p>
                  </div>
                  {provider.connected ? (
                    <CheckCircle2 className="h-5 w-5 text-[var(--success)]" />
                  ) : provider.totalConnections > 0 ? (
                    <CircleAlert className="h-5 w-5 text-[var(--warning)]" />
                  ) : (
                    <XCircle className="h-5 w-5 text-[var(--error)]" />
                  )}
                </div>
              </CardHeader>
              <CardContent className="space-y-3">
                <div className="flex items-center justify-between rounded-md border border-[var(--border)] bg-[var(--secondary)] px-3 py-2 text-sm">
                                  <span>Status</span>
                                  <span className={`font-medium ${provider.connected ? "text-[var(--success)]" : provider.totalConnections > 0 ? "text-[var(--warning)]" : "text-[var(--error)]"}`}>
                                    {provider.connected ? "Connected" : provider.totalConnections > 0 ? "Configured" : "No connections"}
                                  </span>
                                </div>
                <div className="flex items-center justify-between rounded-md border border-[var(--border)] bg-[var(--secondary)] px-3 py-2 text-sm">
                                  <span>Connections</span>
                  <span className="font-medium">{provider.connections} / {provider.totalConnections}</span>
                </div>
                <Button
                  className="w-full"
                  variant={provider.connected ? "outline" : "default"}
                  disabled={busyId === provider.id}
                >
                  {provider.connected ? "Disconnect" : "Connect"}
                  <ChevronRight className="ml-2 h-4 w-4" />
                </Button>
              </CardContent>
            </Card>
          </a>
        ))}
      </div>
    </div>
  );
}