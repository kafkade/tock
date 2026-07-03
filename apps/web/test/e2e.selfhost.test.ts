// TRUE end-to-end smoke test of the self-host path against a REAL `tock-server`.
//
// Opt-in only: set TOCK_E2E=1 to run it. It builds and boots `tock-server` on a
// throwaway SQLite data dir + random port, then drives the REAL `tock-wasm`
// client through the whole first-run flow the browser performs:
//
//   first-run admin creation → save Emergency Kit → SRP login → authed admin call
//
// It also wraps `fetch` to capture every transmitted request body and asserts
// the raw password and Secret Key are never sent — verified against a live
// server, not stubs.
//
//   TOCK_E2E=1 npm test          # run everything including this
//   TOCK_E2E=1 npx vitest run test/e2e.selfhost.test.ts
//
// Skipped by default so the standard `web` CI job stays fast and server-free;
// the self-contained WASM assertions live in smoke.selfhost.test.ts.

import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import init from "tock-wasm";

const RUN_E2E = Boolean(process.env.TOCK_E2E);

const REPO_ROOT = resolve(process.cwd(), "../..");
const SERVER_BIN = resolve(REPO_ROOT, "target/debug/tock-server");
const WASM_PATH = resolve(process.cwd(), "../../crates/tock-wasm/pkg/tock_wasm_bg.wasm");

function freePort(): Promise<number> {
  return new Promise((res, rej) => {
    const srv = createServer();
    srv.on("error", rej);
    srv.listen(0, "127.0.0.1", () => {
      const addr = srv.address();
      if (addr && typeof addr === "object") {
        const { port } = addr;
        srv.close(() => res(port));
      } else {
        srv.close(() => rej(new Error("no port")));
      }
    });
  });
}

async function waitForServer(base: string, timeoutMs = 30_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastErr: unknown;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(`${base}/v1/server/info`);
      if (res.ok) return;
    } catch (err) {
      lastErr = err;
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`server never became ready: ${String(lastErr)}`);
}

describe.skipIf(!RUN_E2E)("self-host e2e against a live tock-server", () => {
  let child: ChildProcess | null = null;
  let dataDir = "";
  let base = "";
  const recorded: { url: string; method: string; body: string }[] = [];
  const realFetch = globalThis.fetch;

  beforeAll(async () => {
    // Build the server binary (cached after the first run).
    execFileSync("cargo", ["build", "-p", "tock-server"], {
      cwd: REPO_ROOT,
      stdio: "inherit",
    });

    await init({ module_or_path: readFileSync(WASM_PATH) });

    const port = await freePort();
    base = `http://127.0.0.1:${port}`;
    dataDir = mkdtempSync(join(tmpdir(), "tock-e2e-"));

    child = spawn(SERVER_BIN, [], {
      cwd: REPO_ROOT,
      env: {
        ...process.env,
        TOCK_BIND: `127.0.0.1:${port}`,
        TOCK_DATA_DIR: dataDir,
        TOCK_MODE: "self-hosted",
        RUST_LOG: "warn",
      },
      stdio: ["ignore", "ignore", "inherit"],
    });

    await waitForServer(base);

    // Record every transmitted request body, then delegate to the real fetch.
    globalThis.fetch = (async (input: RequestInfo | URL, opts?: RequestInit) => {
      const url = typeof input === "string" ? input : input.toString();
      const method = (opts?.method ?? "GET").toUpperCase();
      const body = typeof opts?.body === "string" ? opts.body : "";
      if (body) recorded.push({ url, method, body });
      return realFetch(input, opts);
    }) as typeof fetch;
  }, 600_000);

  afterAll(() => {
    globalThis.fetch = realFetch;
    if (child) child.kill("SIGKILL");
    if (dataDir) rmSync(dataDir, { recursive: true, force: true });
  });

  it("first-run admin → save kit → login → authed call, no secrets sent", async () => {
    const { signupFirstAdmin, isAdmin } = await import("../src/lib/admin");
    const { login, parseSetupCode } = await import("../src/lib/account");

    const email = "admin@example.com";
    const password = "correct horse battery staple 42";

    // Fresh instance: setup is required.
    const info = await (await realFetch(`${base}/v1/server/info`)).json();
    expect(info.setup_required).toBe(true);

    // 1. First-run admin creation against the live server.
    const { bundle, auth, role } = await signupFirstAdmin(base, email, password);
    expect(role).toBe("admin");
    expect(auth.bearerToken).toBeTruthy();
    const secretKey = bundle.secret_key;

    // 2. Save Emergency Kit → Setup Code round-trips to the same account.
    const parsed = await parseSetupCode(bundle.setup_code);
    expect(parsed.email).toBe(email);
    expect(parsed.secret_key).toBe(secretKey);

    // 3. Full SRP login against the live server — real mutual auth.
    const session = await login(base, email, password, secretKey);
    expect(session.bearer_token).toBeTruthy();
    expect(session.channel_binding).toBeTruthy();

    // 4. A basic authenticated admin call succeeds with the login session.
    const admin = await isAdmin(base, {
      bearerToken: session.bearer_token,
      channelBinding: session.channel_binding,
    });
    expect(admin).toBe(true);

    // 5. No transmitted body ever carried the password or Secret Key.
    expect(recorded.length).toBeGreaterThanOrEqual(3);
    for (const r of recorded) {
      expect(r.body).not.toContain(password);
      expect(r.body).not.toContain(secretKey);
    }
  }, 120_000);
});
