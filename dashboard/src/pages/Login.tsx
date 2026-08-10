import { useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Eye, EyeOff, Zap } from "lucide-react";
import { validateApiKey, API_BASE } from "@/lib/api";

interface LoginProps {
  onLogin: () => void;
}

export default function Login({ onLogin }: LoginProps) {
  const [key, setKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!key.trim()) {
      setError("Please enter an API key");
      return;
    }

    setLoading(true);
    setError(null);

    const valid = await validateApiKey(key.trim());
    if (valid) {
      localStorage.setItem("api_key", key.trim());
      onLogin();
    } else {
      setError("Invalid API key");
    }
    setLoading(false);
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-[var(--background)] p-4">
      <Card className="w-full max-w-sm border-[var(--border)] shadow-[var(--shadow-elevated)]">
        <CardHeader className="text-center pb-2">
          <div className="mx-auto mb-3 flex h-14 w-14 items-center justify-center rounded-xl border border-[var(--border)] bg-[var(--card)]">
            <Zap className="h-6 w-6 text-[var(--primary)]" strokeWidth={2.5} />
          </div>
          <CardTitle className="text-2xl font-bold text-[var(--foreground)]">Luminus</CardTitle>
          <p className="text-sm text-[var(--muted-foreground)] mt-2">
            Enter your API key to access the dashboard
          </p>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit} className="space-y-4">
            <div className="relative">
              <Input
                type={showKey ? "text" : "password"}
                value={key}
                onChange={(e) => { setKey(e.target.value); setError(null); }}
                placeholder="sk-pool-..."
                className="pr-10 font-mono text-sm"
                autoFocus
              />
              <button
                type="button"
                onClick={() => setShowKey(!showKey)}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-[var(--muted-foreground)] hover:text-[var(--foreground)]"
              >
                {showKey ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
              </button>
            </div>

            {error && (
              <div className="rounded-md bg-[var(--error)]/10 p-3 text-sm text-[var(--error)]">
                {error}
              </div>
            )}

            <Button type="submit" className="w-full" disabled={loading}>
              {loading ? "Verifying..." : "Login"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
