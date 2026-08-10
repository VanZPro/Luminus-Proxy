import { useEffect, useState } from "react";
import { RefreshCw, Plug, Unplug, TerminalSquare, CheckCircle2, XCircle, CircleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { connectCliTool, disconnectCliTool, fetchCliTools, type CliToolStatus } from "@/lib/api";

const DEFAULT_PATH = "C:\\Users\\Asus\\AppData\\Roaming\\9router\\db\\data.sqlite";

export default function CliTools() {
  const [path, setPath] = useState(DEFAULT_PATH);
  const [tools, setTools] = useState<CliToolStatus[]>([]);
  const [loading, setLoading] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function loadTools() {
    setLoading(true);
    setError(null);
    try {
      const res = await fetchCliTools();
      setTools(res.tools || []);
    } catch (err) {
      setTools([]);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  async function toggleTool(tool: CliToolStatus) {
    setBusyId(tool.id);
    setMessage(null);
    setError(null);
    try {
      if (tool.connected) {
        await disconnectCliTool(tool.id);
        setMessage(`${tool.name} disconnected.`);
      } else {
        await connectCliTool(tool.id);
        setMessage(`${tool.name} connected.`);
      }
      await loadTools();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusyId(null);
    }
  }

  useEffect(() => { loadTools(); }, []);

  const connectedCount = tools.filter((t) => t.connected).length;
  const installedCount = tools.filter((t) => t.installed).length;

  return (
    <div className="space-y-6">
      <div className="rounded-lg border border-[var(--border)] bg-[var(--card)] p-5 shadow-[var(--shadow-card)]">
        <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
          <div>
            <p className="text-xs font-medium uppercase tracking-wider text-[var(--muted-foreground)]">Luminus Tools</p>
            <h1 className="mt-2 text-3xl font-bold text-[var(--foreground)]">CLI Tools</h1>
            <p className="mt-2 text-sm text-[var(--muted-foreground)]">Sync and manage CLI tool status using 9router configuration.</p>
          </div>
          <Button variant="outline" onClick={loadTools} disabled={loading}>
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
          <CardTitle className="flex items-center gap-2"><TerminalSquare className="h-5 w-5" /> 9router database path</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <Input value={path} onChange={(e) => setPath(e.target.value)} className="font-mono text-xs" />
          <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
            <div className="rounded-md border border-[var(--border)] bg-[var(--secondary)] p-3">
                          <div className="text-xl font-bold">{tools.length}</div>
              <div className="text-sm text-[var(--muted-foreground)]">Available tools</div>
            </div>
            <div className="rounded-md border border-[var(--border)] bg-[var(--secondary)] p-3">
                          <div className="text-xl font-bold text-[var(--success)]">{connectedCount}</div>
              <div className="text-sm text-[var(--muted-foreground)]">Connected</div>
            </div>
            <div className="rounded-md border border-[var(--border)] bg-[var(--secondary)] p-3">
                          <div className="text-xl font-bold text-[var(--info)]">{installedCount}</div>
              <div className="text-sm text-[var(--muted-foreground)]">Installed</div>
            </div>
          </div>
        </CardContent>
      </Card>

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-2 2xl:grid-cols-3">
        {tools.map((tool) => (
          <Card key={tool.id} className="overflow-hidden">
            <CardHeader className="pb-3">
              <div className="flex items-start justify-between gap-3">
                <div>
                  <CardTitle>{tool.name}</CardTitle>
                  <p className="mt-1 text-sm text-[var(--muted-foreground)]">{tool.description}</p>
                </div>
                {tool.connected ? (
                  <CheckCircle2 className="h-5 w-5 text-[var(--success)]" />
                ) : tool.installed ? (
                  <CircleAlert className="h-5 w-5 text-[var(--warning)]" />
                ) : (
                  <XCircle className="h-5 w-5 text-[var(--error)]" />
                )}
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="text-xs text-[var(--muted-foreground)]">CLI: <span className="font-mono">{tool.cli}</span></div>
              <div className="text-xs text-[var(--muted-foreground)]">Config: {tool.configPaths[0] || "-"}</div>
              <div className="flex items-center justify-between rounded-md border border-[var(--border)] bg-[var(--secondary)] px-3 py-2 text-sm">
                              <span>Status</span>
                              <span className={`font-medium ${tool.connected ? "text-[var(--success)]" : tool.installed ? "text-[var(--warning)]" : "text-[var(--error)]"}`}>
                                {tool.connected ? "Connected" : tool.installed ? (tool.configured ? "Configured" : "Not configured") : "Not installed"}
                              </span>
                            </div>
              <Button
                className="w-full"
                variant={tool.connected ? "outline" : "default"}
                onClick={() => toggleTool(tool)}
                disabled={busyId === tool.id}
              >
                {tool.connected ? <Unplug className="mr-2 h-4 w-4" /> : <Plug className="mr-2 h-4 w-4" />}
                {busyId === tool.id ? "Working..." : tool.connected ? "Disconnect" : "Connect"}
              </Button>
            </CardContent>
          </Card>
        ))}
      </div>
    </div>
  );
}