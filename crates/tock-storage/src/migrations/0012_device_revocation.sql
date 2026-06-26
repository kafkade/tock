-- Device revocation support.
--
-- A non-NULL `revoked_at` marks a device as revoked. The column is part
-- of the synced `devices` snapshot, so revoking a device on one install
-- propagates to peers on the next sync (the snapshot diff emits a Device
-- Update carrying the new timestamp). Enforcement (rejecting events from
-- revoked signers) is layered on top separately.
ALTER TABLE devices ADD COLUMN revoked_at TEXT;
