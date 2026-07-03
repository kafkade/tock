// Self-contained smoke test of the self-host onboarding + login path, exercised
// with the REAL `tock-wasm` binding (not mocked). It drives the same client
// code the app uses (`admin.ts` / `account.ts`) through a recording `fetch`,
// so every request body the browser would transmit is captured and inspected.
//
// What it proves (issue #176):
//   1. First-run admin creation derives real 2SKD material + an SRP verifier
//      entirely in WASM (no server-side password knowledge).
//   2. The Emergency Kit / Setup Code round-trip locally (save-kit step).
//   3. A full SRP login runs client-side (start → finish proof M1 in WASM), and
//      the client rejects a forged server proof (mutual auth).
//   4. THE security property: the raw password and Secret Key never appear in
//      ANY transmitted request body (register, srp/start, srp/finish).
//
// The server responses are synthesized in-test (no live server); the true
// end-to-end variant against a real `tock-server` lives in e2e.selfhost.test.ts
// and is opt-in behind TOCK_E2E=1.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeAll, describe, expect, it, vi } from "vitest";
import init, { parse_setup_code } from "tock-wasm";

// Load the real wasm once. account.ts/admin.ts call init() with no argument;
// wasm-bindgen's generated init short-circuits when already initialized, so this
// single byte-based init makes the shared module usable under Node/jsdom.
// Vitest runs with cwd = apps/web, so the pkg is two levels up under crates/.
const WASM_PATH = resolve(
  process.cwd(),
  "../../crates/tock-wasm/pkg/tock_wasm_bg.wasm",
);

beforeAll(async () => {
  await init({ module_or_path: readFileSync(WASM_PATH) });
});

interface Recorded {
  url: string;
  method: string;
  body: string;
}

function jsonResponse(obj: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(obj),
    text: () => Promise.resolve(JSON.stringify(obj)),
  } as unknown as Response;
}

describe("self-host onboarding + login (real WASM, no secrets transmitted)", () => {
  it("runs first-run admin → save kit → SRP login entirely client-side", async () => {
    // Import the real client modules (this file does not mock "tock-wasm").
    const { signupFirstAdmin } = await import("../src/lib/admin");
    const { login } = await import("../src/lib/account");

    const email = "admin@example.com";
    const password = "correct horse battery staple 42";

    const recorded: Recorded[] = [];
    let registerBody: {
      srp_salt: string;
      srp_verifier: string;
      kdf_params: unknown;
    } | null = null;

    // Recording fetch: capture every request body, then answer with a
    // plausible server response so the real WASM client keeps running.
    const fetchMock = vi.fn(
      async (url: string, opts: RequestInit = {}): Promise<Response> => {
        const method = (opts.method ?? "GET").toUpperCase();
        const body = typeof opts.body === "string" ? opts.body : "";
        if (body) recorded.push({ url, method, body });

        if (url.endsWith("/v1/accounts/register")) {
          registerBody = JSON.parse(body);
          return jsonResponse({
            account_id: "acct-1",
            role: "admin",
            status: "active",
            admin_token: "adm_interim",
          });
        }
        if (url.endsWith("/v1/auth/srp/start")) {
          // Echo the real kdf_params + salt the client registered with so
          // finish() re-derives the URK and produces a genuine M1 proof. B is a
          // valid non-zero server ephemeral (mutual auth will fail on verify).
          if (!registerBody) throw new Error("srp/start before register");
          const bPub = Buffer.alloc(512, 2).toString("base64");
          return jsonResponse({
            handshake_id: "hs-1",
            salt: registerBody.srp_salt,
            b_pub: bPub,
            kdf_params: registerBody.kdf_params,
          });
        }
        if (url.endsWith("/v1/auth/srp/finish")) {
          // A forged server proof: the client must reject it.
          return jsonResponse({
            m2: Buffer.alloc(32, 7).toString("base64"),
            expires_at: 9_999_999_999,
          });
        }
        throw new Error(`unexpected request: ${method} ${url}`);
      },
    );
    vi.stubGlobal("fetch", fetchMock);

    // 1. First-run admin creation — real 2SKD + verifier derived in WASM.
    const { bundle, auth, role } = await signupFirstAdmin("", email, password);
    expect(role).toBe("admin");
    expect(auth.bearerToken).toBe("adm_interim");

    const secretKey = bundle.secret_key;
    expect(secretKey).toMatch(/^A4-/); // a real, freshly generated Secret Key
    expect(bundle.emergency_kit_text).toContain(secretKey);

    // 2. Save the Emergency Kit → the Setup Code decodes back to the same
    //    account (this is the "save kit" checkpoint the wizard gates on).
    const parsed = (await parse_setup_code(bundle.setup_code)) as {
      email: string;
      secret_key: string;
    };
    expect(parsed.email).toBe(email);
    expect(parsed.secret_key).toBe(secretKey);

    // 3. SRP login runs client-side; the forged server proof is rejected.
    await expect(login("", email, password, secretKey)).rejects.toThrow();

    // The register body carries a derived verifier + salt + kdf params, proving
    // real crypto ran — but never the password or Secret Key themselves.
    const reg = recorded.find((r) => r.url.endsWith("/v1/accounts/register"));
    expect(reg).toBeDefined();
    const regJson = JSON.parse(reg!.body);
    expect(regJson.srp_verifier).toBeTruthy();
    expect(regJson.srp_salt).toBeTruthy();
    expect(regJson.kdf_params).toBeTruthy();

    // All three legs of the flow were exercised.
    const urls = recorded.map((r) => r.url);
    expect(urls.some((u) => u.endsWith("/v1/accounts/register"))).toBe(true);
    expect(urls.some((u) => u.endsWith("/v1/auth/srp/start"))).toBe(true);
    expect(urls.some((u) => u.endsWith("/v1/auth/srp/finish"))).toBe(true);

    // 4. THE security assertion: no transmitted body leaks the password or the
    //    Secret Key. Both are needed to sign in, and neither must ever be sent.
    expect(recorded.length).toBeGreaterThanOrEqual(3);
    for (const r of recorded) {
      expect(r.body).not.toContain(password);
      expect(r.body).not.toContain(secretKey);
    }
  });
});
