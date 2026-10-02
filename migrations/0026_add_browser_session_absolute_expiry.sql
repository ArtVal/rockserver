-- Long-lived browser sessions: a 30-day sliding idle window renewed by
-- authenticated traffic, capped by an absolute lifetime from the sign-in.
-- Existing rows keep their current expiry as the absolute cap, so no already
-- active session is silently extended by this migration; only new passkey
-- sign-ins receive the 180-day absolute ceiling.
ALTER TABLE browser_sessions ADD COLUMN absolute_expires_at timestamptz;
UPDATE browser_sessions SET absolute_expires_at = expires_at WHERE absolute_expires_at IS NULL;
ALTER TABLE browser_sessions ALTER COLUMN absolute_expires_at SET NOT NULL;
