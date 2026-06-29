// Account HTTP client for the tock web app. HTTP lives here (per ADR-001); the
// SRP/2SKD math lives in `tock-wasm`. Mirrors the CLI/UniFFI behaviour exactly.

import init, {
  signup_account,
  parse_setup_code,
  LoginSession,
} from "tock-wasm";

let wasmReady: Promise<unknown> | null = null;

/** Initialise the wasm module once. */
export function ensureWasm(): Promise<unknown> {
  if (!wasmReady) wasmReady = init();
  return wasmReady;
}

export interface SignupBundle {
  register_request_json: string;
  emergency_kit_text: string;
  setup_code: string;
  secret_key: string;
}

export interface ParsedSetupCode {
  server_url: string;
  email: string;
  secret_key: string;
}

export interface Session {
  bearer_token: string;
  channel_binding: string;
  expires_at: number;
}

function trimBase(url: string): string {
  return url.replace(/\/+$/, "");
}

/** Sign up: derive material locally, register on the server, return artifacts. */
export async function signup(
  serverURL: string,
  email: string,
  password: string,
): Promise<SignupBundle> {
  await ensureWasm();
  const bundle = signup_account(email, password, serverURL) as SignupBundle;
  const res = await fetch(`${trimBase(serverURL)}/v1/accounts/register`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: bundle.register_request_json,
  });
  if (!res.ok) {
    throw new Error(`registration failed (${res.status})`);
  }
  return bundle;
}

/** Decode a TOCK1 Setup Code into prefill fields. */
export async function parseSetupCode(code: string): Promise<ParsedSetupCode> {
  await ensureWasm();
  return parse_setup_code(code) as ParsedSetupCode;
}

/** Log in via SRP-6a: start → finish → verify. Returns session material. */
export async function login(
  serverURL: string,
  email: string,
  password: string,
  secretKey: string,
): Promise<Session> {
  await ensureWasm();
  const base = trimBase(serverURL);
  const session = new LoginSession(email);

  const startRes = await fetch(`${base}/v1/auth/srp/start`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: session.start_request_json,
  });
  if (!startRes.ok) throw new Error(`srp/start failed (${startRes.status})`);
  const startJSON = await startRes.text();

  const finishBody = session.finish(startJSON, password, secretKey);
  const finishRes = await fetch(`${base}/v1/auth/srp/finish`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: finishBody,
  });
  if (!finishRes.ok) throw new Error(`srp/finish failed (${finishRes.status})`);
  const finishJSON = await finishRes.text();

  return session.verify(finishJSON) as Session;
}

/** Authenticated request headers used on every sync call. */
export function authHeaders(s: Session): Record<string, string> {
  return {
    Authorization: `Bearer ${s.bearer_token}`,
    "X-Tock-Channel-Binding": s.channel_binding,
  };
}
