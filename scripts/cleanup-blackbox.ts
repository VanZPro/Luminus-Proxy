import { Database } from "bun:sqlite";
import { mkdirSync, copyFileSync, existsSync } from "node:fs";

const dbPath = "data/poolprox3.db";
if (!existsSync(dbPath)) {
  console.error("DB not found:", dbPath);
  process.exit(1);
}

const stamp = new Date().toISOString().replace(/[:.]/g, "-");
mkdirSync("data/backups", { recursive: true });
const backup = `data/backups/poolprox3-before-blackbox-cleanup-${stamp}.db`;
copyFileSync(dbPath, backup);

const db = new Database(dbPath);
console.log("BACKUP", backup);
console.log("COLUMNS", db.query("PRAGMA table_info(accounts)").all().map((r: any) => r.name).join(","));
console.log("BYOK TOTAL", db.query("SELECT count(*) n FROM accounts WHERE provider='byok'").get());
console.log("BYOK 9ROUTER-SOURCE", db.query("SELECT count(*) n FROM accounts WHERE provider='byok' AND (metadata LIKE '%9router%' OR email LIKE '%9router-import%')").get());
console.log("---SAMPLE BYOK (first 30)---");
const sample = db.query("SELECT id, email, status, enabled, substr(coalesce(metadata,''),1,120) as meta FROM accounts WHERE provider='byok' LIMIT 30").all();
for (const r of sample) console.log(JSON.stringify(r));
console.log("---GROUPS BY email prefix (before #)---");
const groups = db.query("SELECT substr(email,1,instr(email,'#')-1) as grp, count(*) n FROM accounts WHERE provider='byok' AND instr(email,'#')>0 GROUP BY grp ORDER BY n DESC LIMIT 30").all();
for (const r of groups) console.log(JSON.stringify(r));
console.log("---NAMES IN METADATA (key_label / name) sample---");
const names = db.query("SELECT metadata FROM accounts WHERE provider='byok' LIMIT 5").all() as Array<{ metadata: string | null }>;
for (const r of names) console.log(String(r?.metadata ?? "").slice(0, 200));
db.close();
