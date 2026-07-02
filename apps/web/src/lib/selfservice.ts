// Self-service account client for the tock web app (issue #131). HTTP lives here
// (per ADR-001); all rotation crypto runs in `tock-wasm`. Every call is scoped
// to the signed-in user's own account via their session bearer token.

import { rotate_password, build_setup_code } from "tock-wasm";
import { ensureWasm, type Session } from "./account";

/** Session auth for self-service calls. Channel binding is optional (these
 * account-scoped routes don't require it, unlike the event routes). */
export interface SessionAuth {
  bearerToken: string;
  channelBinding?: string;
}

/** A registered device row from `GET /v1/account/devices`. */
export interface DeviceItem {
  id: string;
  label?: string;
  registered_at: string;
  revoked: boolean;
}

/** A live session row from `GET /v1/account/sessions`. */
export interface SessionItem {
  id: string;
  created_at: string;
  expires_at: number;
  current: boolean;
}

interface RotationResult {
  new_header_b64: string;
  verifier_update_json: string;
  rewrapped_vk: boolean;
}

function trimBase(url: string): string {
  return url.replace(/\/+$/, "");
}

/** Auth for a logged-in session (used by App/AccountPage). */
export function sessionAuth(s: Session): SessionAuth {
  return { bearerToken: s.bearer_token, channelBinding: s.channel_binding };
}

function authHeaders(auth: SessionAuth): Record<string, string> {
  const h: Record<string, string> = {
    Authorization: ["Bearer", auth.bearerToken].join(" "),
  };
  if (auth.channelBinding) h["X-Tock-Channel-Binding"] = auth.channelBinding;
  return h;
}

async function asError(res: Response, what: string): Promise<never> {
  let detail = "";
  try {
    detail = (await res.text()).slice(0, 200);
  } catch {
    /* ignore */
  }
  throw new Error(`${what} failed (${res.status})${detail ? `: ${detail}` : ""}`);
}

/** Fetch the caller's stored vault header (base64), or null if none exists. */
export async function fetchAccountHeader(
  base: string,
  auth: SessionAuth,
): Promise<string | null> {
  const res = await fetch(`${trimBase(base)}/v1/account/header`, {
    headers: authHeaders(auth),
  });
  if (res.status === 404) return null;
  if (!res.ok) return asError(res, "fetch account header");
  const body = (await res.json()) as { header: string };
  return body.header;
}

/**
 * Rotate the account password end-to-end. Fetches the stored vault header,
 * re-derives the new URK and (when a Vault Key is present) re-wraps it under the
 * new password in WASM, then uploads the fresh SRP verifier and re-wrapped
 * header. The Secret Key is unchanged, so the Emergency Kit / Setup Code stay
 * valid. Returns whether a Vault Key was actually re-wrapped.
 */
export async function rotatePassword(
  base: string,
  auth: SessionAuth,
  oldPassword: string,
  newPassword: string,
  secretKey: string,
): Promise<{ rewrappedVk: boolean }> {
  await ensureWasm();
  const header = await fetchAccountHeader(base, auth);
  if (header === null) {
    throw new Error(
      "this account has no stored vault header, so the password cannot be rotated from the browser",
    );
  }
  const result = rotate_password(
    oldPassword,
    newPassword,
    secretKey.trim(),
    header,
  ) as RotationResult;
  const verifierUpdate = JSON.parse(result.verifier_update_json) as Record<
    string,
    unknown
  >;
  const res = await fetch(`${trimBase(base)}/v1/account/srp-verifier`, {
    method: "PUT",
    headers: { ...authHeaders(auth), "Content-Type": "application/json" },
    body: JSON.stringify({ ...verifierUpdate, header: result.new_header_b64 }),
  });
  if (!res.ok) return asError(res, "rotate password");
  return { rewrappedVk: result.rewrapped_vk };
}

/** Re-render the add-device Setup Code purely from local material (no server
 * round-trip): it is a pure function of these fields. */
export async function buildSetupCode(
  serverURL: string,
  email: string,
  secretKey: string,
): Promise<string> {
  await ensureWasm();
  return build_setup_code(serverURL, email, secretKey.trim());
}

/** List the caller's registered devices. */
export async function listDevices(
  base: string,
  auth: SessionAuth,
): Promise<DeviceItem[]> {
  const res = await fetch(`${trimBase(base)}/v1/account/devices`, {
    headers: authHeaders(auth),
  });
  if (!res.ok) return asError(res, "list devices");
  return (await res.json()) as DeviceItem[];
}

/** Revoke one of the caller's devices by id. */
export async function revokeDevice(
  base: string,
  auth: SessionAuth,
  deviceId: string,
): Promise<void> {
  const res = await fetch(
    `${trimBase(base)}/v1/account/devices/${encodeURIComponent(deviceId)}`,
    { method: "DELETE", headers: authHeaders(auth) },
  );
  if (!res.ok) return asError(res, "revoke device");
}

/** List the caller's live sessions (the current one is flagged). */
export async function listSessions(
  base: string,
  auth: SessionAuth,
): Promise<SessionItem[]> {
  const res = await fetch(`${trimBase(base)}/v1/account/sessions`, {
    headers: authHeaders(auth),
  });
  if (!res.ok) return asError(res, "list sessions");
  return (await res.json()) as SessionItem[];
}

/** Revoke a specific session by id. */
export async function revokeSession(
  base: string,
  auth: SessionAuth,
  sessionId: string,
): Promise<void> {
  const res = await fetch(
    `${trimBase(base)}/v1/account/sessions/${encodeURIComponent(sessionId)}`,
    { method: "DELETE", headers: authHeaders(auth) },
  );
  if (!res.ok) return asError(res, "revoke session");
}

/** End every session except the current one. Returns the number revoked. */
export async function revokeOtherSessions(
  base: string,
  auth: SessionAuth,
): Promise<number> {
  const res = await fetch(
    `${trimBase(base)}/v1/account/sessions/revoke-others`,
    { method: "POST", headers: authHeaders(auth) },
  );
  if (!res.ok) return asError(res, "revoke other sessions");
  const body = (await res.json()) as { revoked: number };
  return body.revoked;
}
