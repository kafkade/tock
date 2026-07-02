import { describe, expect, it, vi, beforeEach } from "vitest";

// Mock the wasm module: the self-service client only needs rotate_password and
// build_setup_code, plus the default init export.
vi.mock("tock-wasm", () => ({
  default: () => Promise.resolve({}),
  rotate_password: (
    _old: string,
    _new: string,
    _sk: string,
    _header: string,
  ) => ({
    new_header_b64: "bmV3LWhlYWRlcg==",
    verifier_update_json: JSON.stringify({
      srp_salt: "c2FsdA==",
      srp_verifier: "dmVyaWZpZXI=",
      srp_group: "RFC5054-4096-SHA256",
      kdf_params: { m: 1 },
    }),
    rewrapped_vk: true,
  }),
  build_setup_code: (server: string, email: string, sk: string) =>
    `TOCK1:${server}:${email}:${sk}`,
}));

import {
  rotatePassword,
  buildSetupCode,
  fetchAccountHeader,
  listDevices,
  listSessions,
  revokeDevice,
  revokeSession,
  revokeOtherSessions,
} from "../src/lib/selfservice";

const AUTH = { bearerToken: "sess_tok", channelBinding: "cb_1" };

function jsonResponse(body: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
    text: () => Promise.resolve(JSON.stringify(body)),
  } as unknown as Response;
}

describe("self-service client", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("attaches the bearer token and channel binding", async () => {
    const f = vi.fn().mockResolvedValue(jsonResponse([]));
    vi.stubGlobal("fetch", f);
    await listSessions("", AUTH);
    const [, init] = f.mock.calls[0];
    // Avoid asserting the exact scheme string; check the token is carried.
    expect(String(init.headers.Authorization)).toContain("sess_tok");
    expect(init.headers["X-Tock-Channel-Binding"]).toBe("cb_1");
  });

  it("fetchAccountHeader returns the base64 header", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(jsonResponse({ header: "aGVhZGVy" })),
    );
    expect(await fetchAccountHeader("", AUTH)).toBe("aGVhZGVy");
  });

  it("fetchAccountHeader returns null on 404", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(jsonResponse({}, 404)),
    );
    expect(await fetchAccountHeader("", AUTH)).toBeNull();
  });

  it("rotatePassword GETs the header then PUTs verifier + new header", async () => {
    const f = vi.fn().mockImplementation((url: string, init?: RequestInit) => {
      if (url.endsWith("/v1/account/header")) {
        return Promise.resolve(jsonResponse({ header: "b2xkLWhlYWRlcg==" }));
      }
      if (url.endsWith("/v1/account/srp-verifier") && init?.method === "PUT") {
        return Promise.resolve(jsonResponse({ ok: true }));
      }
      return Promise.resolve(jsonResponse({}, 500));
    });
    vi.stubGlobal("fetch", f);

    const { rewrappedVk } = await rotatePassword(
      "",
      AUTH,
      "old-pw",
      "new-pw",
      "A4-SECRET",
    );
    expect(rewrappedVk).toBe(true);

    const put = f.mock.calls.find(
      ([url, init]) =>
        url.endsWith("/v1/account/srp-verifier") && init?.method === "PUT",
    );
    expect(put).toBeDefined();
    const body = JSON.parse(put![1].body);
    expect(body.srp_verifier).toBe("dmVyaWZpZXI=");
    expect(body.srp_group).toBe("RFC5054-4096-SHA256");
    // The re-wrapped header travels alongside the verifier update.
    expect(body.header).toBe("bmV3LWhlYWRlcg==");
  });

  it("rotatePassword refuses when the account has no stored header", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(jsonResponse({}, 404)),
    );
    await expect(
      rotatePassword("", AUTH, "old", "new", "A4-SECRET"),
    ).rejects.toThrow(/no stored vault header/);
  });

  it("buildSetupCode delegates to wasm", async () => {
    const code = await buildSetupCode("https://s", "a@b.c", "A4-SECRET");
    expect(code).toBe("TOCK1:https://s:a@b.c:A4-SECRET");
  });

  it("listDevices parses rows", async () => {
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue(
          jsonResponse([
            { id: "dead", registered_at: "2026-01-01T00:00:00Z", revoked: false },
          ]),
        ),
    );
    const devices = await listDevices("", AUTH);
    expect(devices).toHaveLength(1);
    expect(devices[0].id).toBe("dead");
  });

  it("revokeDevice DELETEs the device path", async () => {
    const f = vi.fn().mockResolvedValue(jsonResponse({ ok: true }));
    vi.stubGlobal("fetch", f);
    await revokeDevice("", AUTH, "dead beef");
    const [url, init] = f.mock.calls[0];
    expect(url).toBe("/v1/account/devices/dead%20beef");
    expect(init.method).toBe("DELETE");
  });

  it("revokeSession DELETEs the session path", async () => {
    const f = vi.fn().mockResolvedValue(jsonResponse({ ok: true }));
    vi.stubGlobal("fetch", f);
    await revokeSession("", AUTH, "hash123");
    const [url, init] = f.mock.calls[0];
    expect(url).toBe("/v1/account/sessions/hash123");
    expect(init.method).toBe("DELETE");
  });

  it("revokeOtherSessions POSTs and returns the count", async () => {
    const f = vi.fn().mockResolvedValue(jsonResponse({ revoked: 3 }));
    vi.stubGlobal("fetch", f);
    const n = await revokeOtherSessions("", AUTH);
    expect(n).toBe(3);
    const [url, init] = f.mock.calls[0];
    expect(url).toBe("/v1/account/sessions/revoke-others");
    expect(init.method).toBe("POST");
  });
});
