-- Add message_id column to dm_outbox so we can mark messages as delivered
-- when the outbox entry is successfully sent.
ALTER TABLE dm_outbox ADD COLUMN message_id TEXT NOT NULL DEFAULT '';
