import { useEffect, useMemo, useState } from "react";
import { Database, RefreshCw, Download, AlertTriangle, CheckCircle2, CheckSquare, Square, Wrench, Eye } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { preview9RouterMigration, import9RouterMigration, repair9RouterImports } from "@/lib/api";

const DEFAULT_PATH = "C:\\Users\\Asus\\AppData\\Roaming\\9router\\db\\data.sqlite";

export default function Migration() {
  const [path, setPath] = useState(DEFAULT_PATH);
  const [preview, setPreview] = useState<any>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);
  const [importing, setImporting] = useState(false);
  const [repairing, setRepairing] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function loadPreview() {
    setLoading(true);
    setMessage(null);
    setError(null);
    try {
      const data = await preview9RouterMigration(path.trim());
      setPreview(data);
      setSelected(new Set(Object.keys(data.summary.byProvider || {})));
    } catch (err) {
      setPreview(null);
      setSelected(new Set());
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  function toggleProvider(provider: string) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(provider)) next.delete(provider);
      else next.add(provider);
      return next;
    });
  }

  async function runImport() {
    if (selected.size === 0) {
      setError("Select at least one provider to migrate.");
      return;
    }
    if (!confirm(`Migrate ${selected.size} selected provider type(s) to Luminus? The 9router DB remains unchanged.`)) return;

    setImporting(true);
    setMessage(null);
    setError(null);
    try {
      const data = await import9RouterMigration({ sqlitePath: path.trim(), providers: Array.from(selected) });
      setMessage(`Migration finished: ${data.summary.imported} imported, ${data.summary.skipped} skipped, ${data.summary.errors} errors.`);
      await loadPreview();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setImporting(false);
    }
  }

  async function runRepair() {
    if (selected.size === 0) {
      setError("Select at least one provider to repair.");
      return;
    }
    if (!confirm(`Repair ${selected.size} selected provider type(s) from 9router? This will fix broken imports.`)) return;

    setRepairing(true);
    setMessage(null);
    setError(null);
    try {
      const data = await repair9RouterImports({ sqlitePath: path.trim(), providers: Array.from(selected) });
      setMessage(`Repair finished: ${data.repaired} accounts repaired.`);
      await loadPreview();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setRepairing(false);
    }
  }

  useEffect(() => { loadPreview(); }, []);

  const topProviders = useMemo(
    () => Object.entries(preview?.summary?.byProvider || {}).sort((a: any, b: any) => b[1] - a[1]),
    [preview],
  );
  const allSelected = topProviders.length > 0 && selected.size === topProviders.length;
  const selectedConnections = topProviders.reduce((sum, [provider, count]: any) => selected.has(provider) ? sum + Number(count) : sum, 0);

  return (
    <div className="space-y-6">
      <div className="rounded-lg border border-[var(--border)] bg-[var(--card)] p-5 shadow-[var(--shadow-card)]">
        <div className="flex items-start gap-4">
          <div className="h-10 w-10 rounded-md border border-[var(--border)] bg-[var(--secondary)] flex items-center justify-center">
            <Database className="w-5 h-5 text-[var(--primary)]" />
          </div>
          <div>
            <p className="text-xs font-medium uppercase tracking-wider text-[var(--muted-foreground)]">Luminus Migration Hub</p>
            <h1 className="mt-1 text-2xl font-bold text-[var(--foreground)]">9router → Luminus</h1>
            <p className="text-sm text-[var(--muted-foreground)] mt-1.5 max-w-2xl">
              Choose the 9router database, preview its provider inventory, select providers, then migrate into Luminus. Source data is read-only.
            </p>
          </div>
        </div>
      </div>

      {(message || error) && (
        <div className={`rounded-xl p-4 text-sm border ${message ? "bg-[var(--success)]/10 text-[var(--success)] border-[var(--success)]/30" : "bg-[var(--error)]/10 text-[var(--error)] border-[var(--error)]/30"}`}>
          {message || error}
        </div>
      )}

      <Card>
        <CardHeader><CardTitle className="flex items-center gap-2"><Database className="w-5 h-5" /> 1. Input 9router database path</CardTitle></CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-col sm:flex-row gap-2">
            <Input value={path} onChange={(e) => setPath(e.target.value)} className="font-mono text-xs" disabled={loading || importing} />
            <Button onClick={loadPreview} disabled={loading || importing || !path.trim()} variant="outline">
              <RefreshCw className={`w-4 h-4 mr-2 ${loading ? "animate-spin" : ""}`} /> Preview DB
            </Button>
          </div>
          <div className="flex items-start gap-2 rounded-lg bg-[var(--warning)]/10 border border-[var(--warning)]/30 p-3 text-sm text-[var(--warning)]">
            <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
            9router DB will not be deleted or modified. Luminus only inserts new records into its own DB.
          </div>
        </CardContent>
      </Card>

      {preview && (
        <>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <Card className="stat-accent"><CardContent className="p-5"><div className="text-3xl font-bold">{preview.summary.total}</div><div className="text-sm text-[var(--muted-foreground)]">Total connections</div></CardContent></Card>
            <Card className="stat-accent"><CardContent className="p-5"><div className="text-3xl font-bold">{preview.summary.active}</div><div className="text-sm text-[var(--muted-foreground)]">Active connections</div></CardContent></Card>
            <Card className="stat-accent"><CardContent className="p-5"><div className="text-3xl font-bold">{selectedConnections}</div><div className="text-sm text-[var(--muted-foreground)]">Selected for migration/repair</div></CardContent></Card>
          </div>

          <Card>
            <CardHeader>
              <div className="flex items-center justify-between gap-4">
                <CardTitle>Select providers to migrate or repair</CardTitle>
                <Button variant="outline" size="sm" onClick={() => setSelected(allSelected ? new Set() : new Set(topProviders.map(([p]) => p)))}>
                  {allSelected ? "Clear all" : "Select all"}
                </Button>
              </div>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
                {topProviders.map(([provider, count]: any) => {
                  const checked = selected.has(provider);
                  return (
                    <button
                      type="button"
                      key={provider}
                      onClick={() => toggleProvider(provider)}
                      className={`rounded-md border p-3 flex items-center gap-3 text-left transition-colors ${checked ? "border-[var(--primary)] bg-[var(--sidebar-active)]" : "border-[var(--border)] bg-[var(--secondary)] hover:border-[var(--muted-foreground)]"}`}
                    >
                      {checked ? <CheckSquare className="w-4 h-4 text-[var(--primary)] shrink-0" /> : <Square className="w-4 h-4 text-[var(--muted-foreground)] shrink-0" />}
                      <span className="font-mono text-xs text-[var(--foreground)] truncate flex-1">{provider}</span>
                      <span className="text-sm font-semibold text-[var(--primary)]">{count}</span>
                    </button>
                  );
                })}
              </div>
            </CardContent>
          </Card>

          <div className="flex flex-wrap justify-end gap-3">
            <Button variant="outline" onClick={runRepair} disabled={repairing || selected.size === 0}>
              <Wrench className="w-4 h-4 mr-2" />
              {repairing ? "Repairing..." : `Repair ${selectedConnections} accounts`}
            </Button>
            <Button onClick={runImport} disabled={importing || selected.size === 0}>
              {importing ? <RefreshCw className="w-4 h-4 mr-2 animate-spin" /> : <Download className="w-4 h-4 mr-2" />}
              {importing ? "Migrating to Luminus..." : `Migrate ${selectedConnections} connections`}
            </Button>
          </div>
        </>
      )}

      {message && (
        <div className="flex items-center gap-2 rounded-xl border border-green-500/30 bg-green-500/10 p-4 text-sm text-green-600">
          <CheckCircle2 className="h-5 w-5" />
          {message}
        </div>
      )}
    </div>
  );
}
