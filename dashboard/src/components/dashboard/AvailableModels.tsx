import { useEffect, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ChevronDown, ChevronUp, Copy, Check } from "lucide-react";
import { fetchModels } from "@/lib/api";
import { useTimedMessage } from "@/hooks/useTimedMessage";

interface ModelData {
  id: string;
  object: string;
  created: number;
  owned_by: string;
}

export default function AvailableModels() {
  const [models, setModels] = useState<ModelData[]>([]);
  const [loading, setLoading] = useState(true);
  const [isExpanded, setIsExpanded] = useState(false);
  const { message: copiedModel, setMessage: setCopiedModel } = useTimedMessage<string>(null, 1500);

  useEffect(() => {
    fetchModels()
      .then((res: { data: ModelData[] }) => {
        setModels(res.data || []);
      })
      .catch(() => setModels([]))
      .finally(() => setLoading(false));
  }, []);

  async function copyModelId(modelId: string) {
    await navigator.clipboard.writeText(modelId);
    setCopiedModel(modelId);
  }

  if (loading) {
    return (
      <Card className="border-[var(--border)]">
        <CardHeader>
          <CardTitle className="text-lg flex items-center justify-between">
            <span>Available Models</span>
            <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-[var(--primary)]" />
          </CardTitle>
        </CardHeader>
      </Card>
    );
  }

  return (
    <Card className="border-[var(--border)]">
      <CardHeader>
        <CardTitle
          className="text-lg flex items-center justify-between cursor-pointer"
          onClick={() => setIsExpanded(!isExpanded)}
        >
          <span>Available Models</span>
          {isExpanded ? (
            <ChevronUp className="w-5 h-5 text-[var(--muted-foreground)]" />
          ) : (
            <ChevronDown className="w-5 h-5 text-[var(--muted-foreground)]" />
          )}
        </CardTitle>
      </CardHeader>
      {isExpanded && (
        <CardContent className="space-y-4">
          <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-3">
            {models.slice(0, 12).map((model) => (
              <div
                key={model.id}
                className="flex items-center justify-between gap-2 p-2 rounded-md hover:bg-[var(--secondary)] transition-colors"
              >
                <span className="text-sm text-[var(--foreground)] truncate">
                  {model.id}
                </span>
                <button
                  type="button"
                  onClick={() => copyModelId(model.id)}
                  title={`Copy model ID: ${model.id}`}
                  className="p-1 rounded-md hover:bg-[var(--secondary)] transition-colors"
                >
                  {copiedModel === model.id ? (
                    <Check className="w-4 h-4 text-[var(--success)]" />
                  ) : (
                    <Copy className="w-4 h-4 text-[var(--muted-foreground)]" />
                  )}
                </button>
              </div>
            ))}
          </div>
          {models.length === 0 && (
            <p className="text-sm text-[var(--muted-foreground)]">No models available</p>
          )}
        </CardContent>
      )}
    </Card>
  );
}