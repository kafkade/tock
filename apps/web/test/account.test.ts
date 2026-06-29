import { describe, expect, it, vi, beforeEach } from "vitest";

// Mock the wasm module so the test runs without a wasm runtime.
vi.mock("tock-wasm", () => {
  class LoginSession {
    start_request_json = JSON.stringify({ username: "a@b.c", a_pub: "AA" });
    constructor(_u: string) {}
    finish(_s: string, _p: string, _k: string) {
      return JSON.stringify({ handshake_id: "h", m1: "M1" });
    }
    verify(_f: string) {
      return { bearer_token: "be", channel_binding: "cb", expires_at: 99 };
    }
  }
  return {
    default: () => Promise.resolve({}),
    signup_account: () => ({
      register_request_json: JSON.stringify({ username: "a@b.c" }),
      emergency_kit_text: "tock Emergency Kit",
      setup_code: "TOCK1:xyz",
      secret_key: "A4-XXXX",
    }),
    parse_setup_code: () => ({
      server_url: "https://s",
      email: "a@b.c",
      secret_key: "A4-XXXX",
    }),
    LoginSession,
  };
});

import { signup, login, authHeaders } from "../src/lib/account";

describe("account flows", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("signup posts register body and returns artifacts", async () => {
    const f = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", f);
    const b = await signup("https://s/", "a@b.c", "pw");
    expect(b.setup_code).toBe("TOCK1:xyz");
    expect(f).toHaveBeenCalledWith(
      "https://s/v1/accounts/register",
      expect.objectContaining({ method: "POST" }),
    );
  });

  it("login runs srp/start, finish, verify and returns session", async () => {
    const f = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, text: () => Promise.resolve("{}") })
      .mockResolvedValueOnce({ ok: true, text: () => Promise.resolve("{}") });
    vi.stubGlobal("fetch", f);
    const s = await login("https://s", "a@b.c", "pw", "A4-XXXX");
    expect(s.bearer_token).toBe("be");
    expect(authHeaders(s)["X-Tock-Channel-Binding"]).toBe("cb");
  });
});
