import { Database } from "bun:sqlite";
import { readFileSync } from "node:fs";

export interface MigrationResult {
  imported: number;
  skipped: number;
  errors: number;
  details: string[];
}

export interface ProviderMapper {
  mapTokens(data: any): any;
  mapMetadata(data: any, conn: any): any;
  getProvider(): string;
}

class QoderMapper implements ProviderMapper {
  getProvider() { return "qoder"; }

  mapTokens(data: any) {
    // 9router stores accessToken, but Qoder adapter needs personalToken
    const machineId = data.providerSpecificData?.machineId || data.machineId || "9router-import";
    return {
      source: "9router",
      original_provider: "qoder",
      original_id: data.id,
      personalToken: data.accessToken, // KEY FIX: map accessToken to personalToken
      refreshToken: data.refreshToken,
      userId: data.providerSpecificData?.userId || data.userId,
      userName: data.providerSpecificData?.userName || data.userName,
      email: data.email,
      expireTime: data.expiresAt,
      machineId,
      machineToken: data.providerSpecificData?.machineToken || machineId,
      machineType: data.providerSpecificData?.machineType || "9router",
    };
  }

  mapMetadata(data: any, conn: any) {
    return {
      source: "9router",
      imported_at: new Date().toISOString(),
      original_priority: conn.priority,
      name: data.name || conn.name,
      original_id: data.id,
      original_provider: "qoder",
    };
  }
}

class KiroMapper implements ProviderMapper {
  getProvider() { return "kiro"; }

  mapTokens(data: any) {
    return {
      source: "9router",
      original_provider: "kiro",
      original_id: data.id,
      access_token: data.accessToken,
      refresh_token: data.refreshToken,
      profile_arn: data.providerSpecificData?.profileArn || data.providerSpecificData?.profile_arn,
      expires_at: data.expiresAt,
      region: data.providerSpecificData?.region || "us-east-1",
    };
  }

  mapMetadata(data: any, conn: any) {
    return {
      source: "9router",
      imported_at: new Date().toISOString(),
      original_priority: conn.priority,
      name: data.name || conn.name,
      original_id: data.id,
      original_provider: "kiro",
    };
  }
}

class ByokMapper implements ProviderMapper {
  private prefix: string;

  constructor(prefix: string) {
    this.prefix = prefix;
  }

  getProvider() { return "byok"; }

  mapTokens(data: any) {
    const ps = data.providerSpecificData || {};
    return {
      source: "9router",
      original_provider: ps.original_provider || "openai-compatible",
      original_id: data.id,
      base_url: ps.baseUrl || data.baseUrl || "",
      api_key: data.apiKey,
      format: ps.apiType === "chat" ? "openai" : "auto",
      models: data.models || [],
      model_prefix: this.prefix,
      headers: data.headers || {},
      key_label: data.name || "9router-key",
      priority: data.priority || 0,
    };
  }

  mapMetadata(data: any, conn: any) {
    return {
      source: "9router",
      imported_at: new Date().toISOString(),
      original_priority: conn.priority,
      name: data.name || conn.name,
      original_id: data.id,
      original_provider: data.providerSpecificData?.original_provider || "openai-compatible",
    };
  }
}

class XaiMapper implements ProviderMapper {
  getProvider() { return "xai"; }

  mapTokens(data: any) {
    return {
      source: "9router",
      original_provider: "xai",
      original_id: data.id,
      access_token: data.accessToken,
      refresh_token: data.refreshToken,
      expires_at: data.expiresAt,
      api_key: data.apiKey,
      scope: data.scope,
    };
  }

  mapMetadata(data: any, conn: any) {
    return {
      source: "9router",
      imported_at: new Date().toISOString(),
      original_priority: conn.priority,
      name: data.name || conn.name,
      original_id: data.id,
      original_provider: "xai",
    };
  }
}

export class MigrationService {
  private mappers: Map<string, ProviderMapper> = new Map();

  constructor() {
    this.mappers.set("qoder", new QoderMapper());
    this.mappers.set("kiro", new KiroMapper());
    this.mappers.set("xai", new XaiMapper());
    // BYOK mapper created dynamically with prefix
  }

  getMapper(provider: string, prefix?: string): ProviderMapper | null {
    if (provider.startsWith("openai-compatible")) {
      return new ByokMapper(prefix || provider);
    }
    return this.mappers.get(provider) || null;
  }

  preview(sqlitePath: string): { summary: any; connections: any[] } {
    const db = new Database(sqlitePath, { readonly: true });
    const connections = db.prepare(`
      SELECT id, provider, name, email, priority, isActive, data
      FROM providerConnections
    `).all() as any[];

    const summary = {
      total: connections.length,
      active: connections.filter(c => c.isActive).length,
      byProvider: {} as Record<string, number>,
    };

    for (const conn of connections) {
      summary.byProvider[conn.provider] = (summary.byProvider[conn.provider] || 0) + 1;
    }

    db.close();
    return { summary, connections };
  }

  async import(
    sqlitePath: string,
    providers: string[] | undefined,
    db: any,
    accounts: any,
    eq: any,
    encrypt: any
  ): Promise<MigrationResult> {
    const selected = new Set(providers || []);
    const sourceDb = new Database(sqlitePath, { readonly: true });

    const connections = sourceDb.prepare(`
      SELECT id, provider, name, email, priority, isActive, data
      FROM providerConnections
      WHERE isActive = 1
    `).all() as any[];

    const result: MigrationResult = {
      imported: 0,
      skipped: 0,
      errors: 0,
      details: [],
    };

    for (const conn of connections) {
      const originalProvider = conn.provider;

      // Skip if not in selected providers
      if (selected.size > 0 && !selected.has(originalProvider)) {
        result.skipped++;
        continue;
      }

      try {
        const data = JSON.parse(conn.data || "{}");
        const mapper = this.getMapper(originalProvider, data.providerSpecificData?.prefix || conn.name);

        if (!mapper) {
          result.skipped++;
          result.details.push(`SKIP: ${originalProvider}/${conn.id} (no mapper)`);
          continue;
        }

        const tokens = mapper.mapTokens({ ...data, id: conn.id });
        const metadata = mapper.mapMetadata({ ...data, id: conn.id }, conn);
        const astraProvider = mapper.getProvider();

        // Check for duplicates
        const email = conn.email || `${astraProvider}-${conn.id.slice(0, 8)}@9router-import`;
        const existing = await db.select().from(accounts)
          .where(eq(accounts.email, email))
          .then((rows: any[]) => rows.find((r: any) => r.provider === astraProvider));

        if (existing) {
          result.skipped++;
          result.details.push(`SKIP: ${astraProvider}/${email} (already exists)`);
          continue;
        }

        // Insert account
        const secret = data.apiKey || data.accessToken || data.refreshToken || "9router-import";
        await db.insert(accounts).values({
          provider: astraProvider,
          email,
          password: encrypt(secret),
          status: "active",
          enabled: true,
          tokens,
          quotaLimit: -1,
          quotaRemaining: -1,
          metadata,
        });

        result.imported++;
        result.details.push(`OK: ${astraProvider}/${email}`);
      } catch (err) {
        result.errors++;
        result.details.push(`ERROR: ${conn.id} - ${err instanceof Error ? err.message : String(err)}`);
      }
    }

    sourceDb.close();
    return result;
  }
}
