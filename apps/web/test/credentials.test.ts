import { describe, expect, it } from "vitest";
import { MemoryStore, SessionStorageStore } from "../src/lib/credentials";

const sample = {
  serverURL: "https://tock.example",
  email: "a@b.c",
  session: { bearer_token: "ab", channel_binding: "cd", expires_at: 1 },
};

describe("MemoryStore", () => {
  it("round-trips and clears", () => {
    const s = new MemoryStore();
    expect(s.load()).toBeNull();
    s.save(sample);
    expect(s.load()?.email).toBe("a@b.c");
    s.clear();
    expect(s.load()).toBeNull();
  });
});

describe("SessionStorageStore", () => {
  it("round-trips and clears via sessionStorage", () => {
    const s = new SessionStorageStore();
    s.save(sample);
    expect(s.load()?.session.bearer_token).toBe("ab");
    s.clear();
    expect(s.load()).toBeNull();
  });
});
