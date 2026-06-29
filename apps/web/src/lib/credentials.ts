// Web credential store. Defaults to in-memory (cleared on reload) so the bearer
// token and channel binding never touch disk; the password is never stored.
// A sessionStorage tier is offered for convenience within a tab session. The
// account Secret Key is intentionally NOT persisted here (ADR-012): the browser
// is a weaker keystore than an OS keychain, so the user re-enters it or pastes a
// Setup Code on a fresh tab.

import type { Session } from "./account";

export interface StoredCredentials {
  serverURL: string;
  email: string;
  session: Session;
}

const KEY = "tock.session.v1";

export interface CredentialStore {
  save(c: StoredCredentials): void;
  load(): StoredCredentials | null;
  clear(): void;
}

/** In-memory store (default). Cleared on page reload. */
export class MemoryStore implements CredentialStore {
  private value: StoredCredentials | null = null;
  save(c: StoredCredentials) {
    this.value = c;
  }
  load() {
    return this.value;
  }
  clear() {
    this.value = null;
  }
}

/** sessionStorage-backed store (per-tab, weaker; opt-in). */
export class SessionStorageStore implements CredentialStore {
  save(c: StoredCredentials) {
    sessionStorage.setItem(KEY, JSON.stringify(c));
  }
  load(): StoredCredentials | null {
    const raw = sessionStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as StoredCredentials) : null;
  }
  clear() {
    sessionStorage.removeItem(KEY);
  }
}
