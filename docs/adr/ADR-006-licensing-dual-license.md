# ADR-006: Licensing — Apache-2.0 core, AGPL-3.0 server

**Status:** Accepted  
**Date:** 2026-05-20

## Context

Tock's licensing must balance:

1. **Community contribution:** Permissive licensing encourages adoption and contributions.
2. **User freedom:** Users must be able to self-host, fork, and modify without restriction.
3. **SaaS exploitation defense:** Prevent a third party from taking the code, running a closed-source hosted service, and undermining the project's sustainability.
4. **Sustainability:** The project must support a 1–2 person team long-term without compromising its ethos.

Fully permissive licenses (MIT, Apache-2.0) allow SaaS exploitation. Fully copyleft licenses (GPL-3.0) discourage commercial adoption and integration. A single license across all components forces a compromise.

## Decision

**Dual licensing:**

1. **Core, CLI, bindings, and apps: Apache-2.0**
   - `tock-core`, `tock-crypto`, `tock-parse`, `tock-storage`, `tock-sync`, `tock-import`, `tock-export`, `tock-cli`, `tock-uniffi`.
   - Swift bindings, iOS/iPadOS/macOS/watchOS apps.
   - WASM bindings and web client.

2. **Sync server: AGPL-3.0**
   - `tock-server` (Axum-based HTTP sync server).

**Rationale:**
- Apache-2.0 client code allows integration into proprietary apps (e.g., a company embedding Tock's CLI in their internal tools).
- AGPL-3.0 server ensures any third party offering hosted sync as a service must publish their modifications (including deployment scripts, if they constitute a modified version).
- Self-hosting remains free forever (AGPL permits private modifications).
- This prevents a competitor from forking `tock-server`, adding proprietary extensions, and running a closed SaaS without contributing back.

**Compatibility:**
Apache-2.0 is GPL-compatible (clients can link AGPL server code if they choose). AGPL server cannot link Apache-2.0 client libraries without triggering AGPL obligations, but the sync protocol is pure HTTP/JSON, so no linking occurs.

## Consequences

**Positive:**
- Client code is maximally permissive (encourages adoption, corporate use, forks).
- AGPL server prevents SaaS exploitation (competitors must open-source their hosted sync forks).
- Self-hosting is free and unencumbered (AGPL allows private modifications).
- Clear licensing split aligns with Tock's open core + paid hosted sync business model.

**Negative:**
- Dual licensing adds complexity (contributors must understand which components fall under which license).
- AGPL may deter some corporate adopters from self-hosting (mitigated by offering hosted sync as a paid service).

**Neutral:**
- Enterprise users wanting proprietary modifications to the server can negotiate a commercial license (future revenue opportunity).
- Documentation must clearly explain the licensing split (`LICENSE-APACHE-2.0` and `LICENSE-AGPL-3.0` in repo root, per-crate `README.md` specifies which applies).
