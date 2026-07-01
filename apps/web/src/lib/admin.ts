// Admin + first-run client for the tock web console. HTTP lives here (per
// ADR-001); all SRP/2SKD crypto runs in `tock-wasm`. The console is served
// same-origin behind the reverse proxy, so `base` defaults to "" (relative
// fetch); an explicit URL is still accepted for dev against a remote server.

import { signup_account } from "tock-wasm";
import { ensureWasm, type SignupBundle } from "./account";

/** Public instance metadata from `GET /v1/server/info`. */
export interface ServerInfo {
  setup_required: boolean;
  registration_policy: string;
  mode: string;
  version: string;
}

/** A user row from the admin API. */
export interface AdminUser {
  id: string;
  username: string;
  role: string;
  status: string;
  created_at: string;
}

/** Bearer credential for admin calls: either the interim admin token minted at
 * first-run signup, or a full SRP login session. Channel binding is optional —
 * admin endpoints accept the interim token without it. */
export interface AdminAuth {
  bearerToken: string;
  channelBinding?: string;
}

/** Result of bootstrapping the first admin account. */
export interface AdminSignup {
  bundle: SignupBundle;
  auth: AdminAuth;
  role: string;
}

function trimBase(url: string): string {
  return url.replace(/\/+$/, "");
}

function authHeaders(auth: AdminAuth): Record<string, string> {
  const h: Record<string, string> = {
    Authorization: `Bearer ${auth.bearerToken}`,
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

/** Fetch public instance metadata (unauthenticated). */
export async function fetchServerInfo(base = ""): Promise<ServerInfo> {
  const res = await fetch(`${trimBase(base)}/v1/server/info`);
  if (!res.ok) return asError(res, "server info");
  return (await res.json()) as ServerInfo;
}

/** Bootstrap the first admin on a fresh instance: derive material locally,
 * register, and capture the interim admin token from the response. */
export async function signupFirstAdmin(
  base: string,
  email: string,
  password: string,
): Promise<AdminSignup> {
  await ensureWasm();
  const serverURL = trimBase(base) || window.location.origin;
  const bundle = signup_account(email, password, serverURL) as SignupBundle;
  const res = await fetch(`${trimBase(base)}/v1/accounts/register`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: bundle.register_request_json,
  });
  if (!res.ok) return asError(res, "admin registration");
  const body = (await res.json()) as {
    role: string;
    admin_token?: string | null;
  };
  if (!body.admin_token) {
    throw new Error(
      "instance did not return an admin token — an account may already exist",
    );
  }
  return {
    bundle,
    role: body.role,
    auth: { bearerToken: body.admin_token },
  };
}

/** Probe whether the given credential has admin rights (200 vs 403). */
export async function isAdmin(base: string, auth: AdminAuth): Promise<boolean> {
  const res = await fetch(`${trimBase(base)}/v1/admin/settings`, {
    headers: authHeaders(auth),
  });
  if (res.status === 200) return true;
  if (res.status === 401 || res.status === 403) return false;
  return asError(res, "admin probe");
}

/** List all accounts. */
export async function listUsers(
  base: string,
  auth: AdminAuth,
): Promise<AdminUser[]> {
  const res = await fetch(`${trimBase(base)}/v1/admin/users`, {
    headers: authHeaders(auth),
  });
  if (!res.ok) return asError(res, "list users");
  return (await res.json()) as AdminUser[];
}

/** Mint an invite for a new account (admins cannot set passwords). */
export async function createInvite(
  base: string,
  auth: AdminAuth,
  opts: { username?: string; role?: "user" | "admin" } = {},
): Promise<{ invite_token: string; role: string; username?: string }> {
  const res = await fetch(`${trimBase(base)}/v1/admin/users`, {
    method: "POST",
    headers: { ...authHeaders(auth), "Content-Type": "application/json" },
    body: JSON.stringify({
      username: opts.username || undefined,
      role: opts.role || "user",
    }),
  });
  if (!res.ok) return asError(res, "create invite");
  return (await res.json()) as {
    invite_token: string;
    role: string;
    username?: string;
  };
}

/** Enable or disable an account. */
export async function setUserEnabled(
  base: string,
  auth: AdminAuth,
  accountId: string,
  enabled: boolean,
): Promise<void> {
  const action = enabled ? "enable" : "disable";
  const res = await fetch(
    `${trimBase(base)}/v1/admin/users/${encodeURIComponent(accountId)}/${action}`,
    { method: "POST", headers: authHeaders(auth) },
  );
  if (!res.ok) return asError(res, `${action} user`);
}

/** Delete an account. */
export async function deleteUser(
  base: string,
  auth: AdminAuth,
  accountId: string,
): Promise<void> {
  const res = await fetch(
    `${trimBase(base)}/v1/admin/users/${encodeURIComponent(accountId)}`,
    { method: "DELETE", headers: authHeaders(auth) },
  );
  if (!res.ok) return asError(res, "delete user");
}

/** Read the current registration policy. */
export async function getRegistrationPolicy(
  base: string,
  auth: AdminAuth,
): Promise<string> {
  const res = await fetch(`${trimBase(base)}/v1/admin/settings`, {
    headers: authHeaders(auth),
  });
  if (!res.ok) return asError(res, "get settings");
  const body = (await res.json()) as { registration_policy: string };
  return body.registration_policy;
}

/** Update the registration policy (`open`, `invite-only`, `disabled`). */
export async function setRegistrationPolicy(
  base: string,
  auth: AdminAuth,
  policy: string,
): Promise<string> {
  const res = await fetch(`${trimBase(base)}/v1/admin/settings`, {
    method: "PUT",
    headers: { ...authHeaders(auth), "Content-Type": "application/json" },
    body: JSON.stringify({ registration_policy: policy }),
  });
  if (!res.ok) return asError(res, "set settings");
  const body = (await res.json()) as { registration_policy: string };
  return body.registration_policy;
}
