// Simulates the production src/index.ts middleware + src/auth/index.ts guard
// using the real keys module logic. Validates public-key blocking on /api/auth/*.
import { Hono } from "hono";
import { isManagementAllowed } from "../src/api/keys";

// Fake auth routes mirroring src/auth/index.ts
const authRouter = new Hono();
authRouter.use("*", async (c, next) => {
  const apiKeyRow = c.get("apiKeyRow");
  if (apiKeyRow === undefined) {
    return c.json({ error: { message: "Unauthorized", type: "auth_error" } }, 401);
  }
  if (!isManagementAllowed(apiKeyRow)) {
    return c.json({
      error: {
        message: "This API key is restricted to public proxy use only and cannot access authentication endpoints.",
        type: "forbidden",
        code: "public_only_key",
      },
    }, 403);
  }
  await next();
});
authRouter.post("/login/:id", (c) => c.json({ message: "Login queued", accountId: c.req.param("id") }));

const proxyRouter = new Hono();
proxyRouter.get("/v1/models", (c) => c.json({ models: [] }));

// Mirror production index.ts middleware + mount order
const app = new Hono();

// 1) proxy key middleware (simplified validity)
app.use("/v1/*", async (c, next) => {
  const token = (c.req.header("Authorization") || "").replace("Bearer ", "");
  if (token === "admin-key" || token === "public-key") { await next(); return; }
  return c.json({ error: { message: "Invalid API key" } }, 401);
});

// 2) management /api/* middleware w/ isManagementAllowed
app.use("/api/*", async (c, next) => {
  if (c.req.path === "/api/health" || c.req.path === "/api/info" || c.req.path === "/api/keys/test") { await next(); return; }
  const token = (c.req.header("Authorization") || "").replace("Bearer ", "");
  // legacy global key (null row) => allowed; simulate resolveApiKey by token
  let resolved;
  if (token === "admin-key") {
    resolved = { row: { isPublicOnly: false } };
  } else if (token === "public-key") {
    resolved = { row: { isPublicOnly: true } };
  } else if (token && token !== "public-key") {
    // legacy global key => row null
    resolved = { row: null };
  } else {
    resolved = { row: undefined };
  }
  if (!token || resolved.row === undefined) {
    return c.json({ error: { message: "Unauthorized", type: "auth_error" } }, 401);
  }
  if (!isManagementAllowed(resolved.row)) {
    return c.json({ error: { message: "This API key is restricted...", code: "public_only_key" } }, 403);
  }
  c.set("apiKeyRow", resolved.row);
  await next();
});

// Mount routes AFTER middleware
app.route("/", proxyRouter);
app.route("/api/auth", authRouter);

async function t(name, method, path, header) {
  const headers = header ? { Authorization: header } : {};
  const req = new Request(`http://x${path}`, { method, headers });
  const res = await app.fetch(req);
  console.log(`${name.padEnd(32)} => ${res.status}  ${await res.text().catch(() => '')}`);
}

console.log("Scenario A - public-only API key hits /api/auth/login => expect 403");
await t("A1 public key login", "POST", "/api/auth/login/1", "Bearer public-key");
console.log("Scenario B - no key /api/auth/login => expect 401");
await t("B1 no key login", "POST", "/api/auth/login/1", undefined);
console.log("Scenario C - admin key /api/auth/login => expect 200");
await t("C1 admin key login", "POST", "/api/auth/login/1", "Bearer admin-key");
console.log("Scenario D - public key on /v1/models (allowed) => expect 200");
await t("D1 public key models", "GET", "/v1/models", "Bearer public-key");
console.log("Scenario E - admin key on /v1/models (allowed) => expect 200");
await t("E1 admin key models", "GET", "/v1/models", "Bearer admin-key");
console.log("Scenario F - invalid key on /v1/models => expect 401");
await t("F1 invalid key models", "GET", "/v1/models", "Bearer invalid-key");
console.log("Scenario G - public key on /api/auth/login-bulk => expect 403");
await t("G1 public key login-bulk", "POST", "/api/auth/login-bulk", "Bearer public-key");
console.log("Scenario H - public key on /api/auth/queue (GET) => expect 403");
await t("H1 public key queue", "GET", "/api/auth/queue", "Bearer public-key");
console.log("Scenario I - admin key on /api/auth/queue => expect 200");
await t("I1 admin key queue", "GET", "/api/auth/queue", "Bearer admin-key");