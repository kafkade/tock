import { describe, expect, it, vi, beforeEach } from "vitest";

// Mock the wasm module so tests run without a wasm runtime. Only the pieces the
// admin client touches (signup_account) are needed here.
vi.mock("tock-wasm", () => ({
  default: () => Promise.resolve({}),
  signup_account: () => ({
    register_request_json: JSON.stringify({ username: "admin@example.com" }),
    emergency_kit_text: "tock Emergency Kit",
    setup_code: "TOCK1:xyz",
    secret_key: "A4-XXXX",
  }),
}));

import {
  fetchServerInfo,
  signupFirstAdmin,
  isAdmin,
  listUsers,
  createInvite,
  setRegistrationPolicy,
  getSettings,
  updateSettings,
  getStats,
} from "../src/lib/admin";

const AUTH = { bearerToken: "adm_token" };

function jsonResponse(body: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
    text: () => Promise.resolve(JSON.stringify(body)),
  } as unknown as Response;
}

describe("admin client", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("fetchServerInfo parses instance metadata (unauthenticated)", async () => {
    const f = vi.fn().mockResolvedValue(
      jsonResponse({
        setup_required: true,
        registration_policy: "disabled",
        mode: "self-hosted",
        version: "0.4.0",
      }),
    );
    vi.stubGlobal("fetch", f);

    const info = await fetchServerInfo("");
    expect(info.setup_required).toBe(true);
    expect(info.mode).toBe("self-hosted");
    // Same-origin: relative URL, no Authorization header.
    const [url, init] = f.mock.calls[0];
    expect(url).toBe("/v1/server/info");
    expect(init).toBeUndefined();
  });

  it("signupFirstAdmin registers and returns the interim admin token", async () => {
    const f = vi
      .fn()
      .mockResolvedValue(jsonResponse({ role: "admin", admin_token: "adm_1" }));
    vi.stubGlobal("fetch", f);

    const result = await signupFirstAdmin("", "admin@example.com", "pw");
    expect(result.role).toBe("admin");
    expect(result.auth.bearerToken).toBe("adm_1");
    expect(result.bundle.setup_code).toBe("TOCK1:xyz");
    const [url, init] = f.mock.calls[0];
    expect(url).toBe("/v1/accounts/register");
    expect(init.method).toBe("POST");
  });

  it("signupFirstAdmin throws when no admin token is returned", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(jsonResponse({ role: "user" })),
    );
    await expect(
      signupFirstAdmin("", "second@example.com", "pw"),
    ).rejects.toThrow(/admin token/);
  });

  it("isAdmin maps 200 to true and 403 to false", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(jsonResponse({}, 200)));
    expect(await isAdmin("", AUTH)).toBe(true);

    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(jsonResponse({ error: "no" }, 403)),
    );
    expect(await isAdmin("", AUTH)).toBe(false);
  });

  it("listUsers sends the bearer token", async () => {
    const f = vi
      .fn()
      .mockResolvedValue(jsonResponse([{ id: "1", username: "a" }]));
    vi.stubGlobal("fetch", f);

    const users = await listUsers("", AUTH);
    expect(users).toHaveLength(1);
    const [, init] = f.mock.calls[0];
    expect(init.headers.Authorization).toBe("Bearer adm_token");
  });

  it("createInvite defaults role to user and posts the body", async () => {
    const f = vi
      .fn()
      .mockResolvedValue(jsonResponse({ invite_token: "inv_1", role: "user" }));
    vi.stubGlobal("fetch", f);

    const r = await createInvite("", AUTH, { username: "bob" });
    expect(r.invite_token).toBe("inv_1");
    const [, init] = f.mock.calls[0];
    expect(JSON.parse(init.body)).toEqual({ username: "bob", role: "user" });
  });

  it("setRegistrationPolicy PUTs the new policy and returns it", async () => {
    const f = vi
      .fn()
      .mockResolvedValue(jsonResponse({ registration_policy: "open" }));
    vi.stubGlobal("fetch", f);

    const p = await setRegistrationPolicy("", AUTH, "open");
    expect(p).toBe("open");
    const [url, init] = f.mock.calls[0];
    expect(url).toBe("/v1/admin/settings");
    expect(init.method).toBe("PUT");
  });

  it("getSettings reads policy + public address", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        jsonResponse({
          registration_policy: "invite-only",
          public_address: "https://tock.example.com",
        }),
      ),
    );
    const s = await getSettings("", AUTH);
    expect(s.registration_policy).toBe("invite-only");
    expect(s.public_address).toBe("https://tock.example.com");
  });

  it("updateSettings PATCHes only the provided public address", async () => {
    const f = vi.fn().mockResolvedValue(
      jsonResponse({
        registration_policy: "invite-only",
        public_address: "https://new.example.com",
      }),
    );
    vi.stubGlobal("fetch", f);
    const s = await updateSettings("", AUTH, {
      public_address: "https://new.example.com",
    });
    expect(s.public_address).toBe("https://new.example.com");
    const [url, init] = f.mock.calls[0];
    expect(url).toBe("/v1/admin/settings");
    expect(init.method).toBe("PUT");
    expect(JSON.parse(init.body)).toEqual({
      public_address: "https://new.example.com",
    });
  });

  it("getStats reads instance counters", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        jsonResponse({
          accounts_total: 5,
          accounts_admin: 1,
          accounts_active: 4,
          accounts_disabled: 1,
          vaults: 3,
          devices: 7,
          events: 42,
          storage_bytes: 4096,
        }),
      ),
    );
    const st = await getStats("", AUTH);
    expect(st.accounts_total).toBe(5);
    expect(st.storage_bytes).toBe(4096);
  });
});
