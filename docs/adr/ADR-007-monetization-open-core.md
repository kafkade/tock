# ADR-007: Monetization — Open core with paid hosted sync

**Status:** Accepted  
**Date:** 2026-05-20

## Context

Tock must sustain a 1–2 person team indefinitely without betraying its privacy-first, open-source ethos. Options:

- **A: Fully open source, no monetization** → Unsustainable (relies on donations, which historically fail for productivity tools).
- **B: Open core + paid hosted sync** → Server is AGPL; hosted sync is a paid service; all client code is free.
- **C: Freemium apps** → CLI free, Apple apps paid → Hard to justify when apps are Apache-2.0 (anyone can fork and redistribute).
- **D: Paid features** → Pro tier unlocks advanced features → Feature gating conflicts with open-source principles.

We need a model that sustains development while keeping all code open, never gating features, and offering a genuine free tier.

## Decision

**Model B: Open core with paid hosted sync**

**All code is open source:**
- Core, CLI, bindings, apps: Apache-2.0 (free forever).
- Sync server: AGPL-3.0 (free to self-host forever).

**Hosted sync as a paid service:**
- **Free tier:** 2 devices, 10,000 events/month, 90-day history retention.
- **Personal ($5/month or $48/year):** 5 devices, unlimited events, 2-year retention, priority support.
- **Family ($12/month or $120/year):** 10 devices, shared vaults, 5-year retention.
- **Pro ($20/month or $200/year):** Unlimited devices, 10-year retention, API access, white-glove onboarding, SLA.

**Apple apps:** Free on the App Store (no paid download, no in-app purchases for features). Apps are fully functional offline and with self-hosted sync.

**CLI:** Free, unlimited, open source. Power users can self-host sync and never pay.

**Transparency:**
- Pricing page clearly states: "All code is open source. Self-hosting is free forever. You're paying for hosting, not for code."
- Self-hosting instructions prominently documented.

## Consequences

**Positive:**
- Sustainable revenue stream (hosted sync covers server costs + developer time).
- Free tier is generous (most individual users fit within it).
- No dark patterns (free features never become paid; pricing is transparent).
- Self-hosting remains a first-class experience (AGPL ensures competitors can't undercut by forking).
- CLI users can self-host indefinitely without paying (respects power users).

**Negative:**
- Hosted sync revenue depends on user growth (vs. one-time app sales).
- Free tier costs must be carefully managed (10,000 events/month is ~100 KiB; storage cost is negligible, but support cost scales with user count).
- Family and Pro tiers may not attract enough users initially (acceptable; Personal tier is the core revenue driver).

**Neutral:**
- Competitors can fork and offer their own hosted sync (AGPL means they must publish modifications, leveling the field).
- Enterprise sales (custom SLA, on-premise deployment support, commercial license for server modifications) are a future opportunity.
