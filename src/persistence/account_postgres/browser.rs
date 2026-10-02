//! Private PostgreSQL account-store operations grouped by one responsibility.

use super::rows::*;
use super::*;
use crate::auth::{BROWSER_SESSION_ABSOLUTE_MINUTES, BROWSER_SESSION_IDLE_MINUTES};

impl PostgresAccountStore {
    /// Creates a browser session with the standard sliding/absolute lifetimes,
    /// both measured from PostgreSQL's clock.
    pub async fn create_browser_session_with_policy(
        &self,
        session: NewBrowserSession<'_>,
    ) -> Result<bool, sqlx::Error> {
        let inserted = sqlx::query(
            "INSERT INTO browser_sessions (id, user_id, session_token_hash, csrf_token_hash, passkey_reauthenticated_at, expires_at, absolute_expires_at) \
             SELECT $1, id, $3, $4, now(), now() + ($5 * interval '1 minute'), now() + ($6 * interval '1 minute') \
             FROM users WHERE id = $2 AND status = 'active'",
        )
        .bind(session.session_id)
        .bind(session.user_id)
        .bind(session.session_token_hash.as_bytes())
        .bind(session.csrf_hash.as_bytes())
        .bind(BROWSER_SESSION_IDLE_MINUTES)
        .bind(BROWSER_SESSION_ABSOLUTE_MINUTES)
        .execute(&self.pool)
        .await?;
        Ok(inserted.rows_affected() == 1)
    }

    /// Revokes a browser session without exposing whether it was already revoked.
    pub async fn revoke_browser_session(&self, session_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE browser_sessions SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL",
        )
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Renews the sliding idle window of a live session, capped by its absolute expiry.
    ///
    /// The statement only writes when less than half of the idle window remains,
    /// so ordinary authenticated traffic costs one cheap indexed lookup and no
    /// write most of the time. Best effort by design: callers ignore errors so
    /// a renewal hiccup never fails an authenticated request.
    pub async fn renew_browser_session(
        &self,
        session_token_hash: &SecretHash,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE browser_sessions SET expires_at = LEAST(now() + ($2 * interval '1 minute'), absolute_expires_at) \
             WHERE session_token_hash = $1 AND revoked_at IS NULL AND expires_at > now() \
             AND expires_at < now() + ($2 * interval '1 minute') * 0.5",
        )
        .bind(session_token_hash.as_bytes())
        .bind(BROWSER_SESSION_IDLE_MINUTES)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Resolves an active browser session to its owner only when its current CSRF proof matches.
    ///
    /// A successful resolve also renews the session's sliding idle window
    /// (see [`Self::renew_browser_session`]); renewal failures are logged and ignored.
    pub async fn browser_session_user_with_csrf(
        &self,
        session_token_hash: &SecretHash,
        csrf_hash: &SecretHash,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        let user_id = sqlx::query_scalar(
            "SELECT b.user_id FROM browser_sessions b JOIN users u ON u.id = b.user_id \
             WHERE b.session_token_hash = $1 AND b.csrf_token_hash = $2 AND b.revoked_at IS NULL \
             AND b.expires_at > now() AND b.absolute_expires_at > now() AND u.status = 'active'",
        )
        .bind(session_token_hash.as_bytes())
        .bind(csrf_hash.as_bytes())
        .fetch_optional(&self.pool)
        .await?;
        if user_id.is_some()
            && let Err(error) = self.renew_browser_session(session_token_hash).await
        {
            tracing::warn!(error = %error, "browser session renewal failed");
        }
        Ok(user_id)
    }

    /// Resolves a live browser cookie to its owner without exposing that identifier to HTTP clients.
    ///
    /// A successful resolve also renews the session's sliding idle window;
    /// renewal failures are logged and ignored (see [`Self::renew_browser_session`]).
    pub async fn browser_session_user(
        &self,
        session_token_hash: &SecretHash,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        let user_id = sqlx::query_scalar(
            "SELECT b.user_id FROM browser_sessions b JOIN users u ON u.id = b.user_id \
             WHERE b.session_token_hash = $1 AND b.revoked_at IS NULL AND b.expires_at > now() \
             AND b.absolute_expires_at > now() AND u.status = 'active'",
        )
        .bind(session_token_hash.as_bytes())
        .fetch_optional(&self.pool)
        .await?;
        if user_id.is_some()
            && let Err(error) = self.renew_browser_session(session_token_hash).await
        {
            tracing::warn!(error = %error, "browser session renewal failed");
        }
        Ok(user_id)
    }

    /// Lists only safe device metadata for the owner of an already authenticated browser session.
    pub async fn list_browser_devices(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<BrowserDevice>, sqlx::Error> {
        sqlx::query_as::<_, BrowserDeviceRow>(
            "SELECT d.id, d.device_display_name, d.device_type, \
             to_char(d.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at, \
             to_char(GREATEST(d.last_seen_at, (SELECT max(s.last_seen_at) FROM sessions s WHERE s.device_id = d.id)) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS last_seen_at, \
             CASE WHEN EXISTS (SELECT 1 FROM sessions s WHERE s.device_id = d.id AND s.revoked_at IS NULL AND s.access_expires_at > now()) THEN 'active' ELSE 'inactive' END AS session_status \
             FROM devices d JOIN users u ON u.id = d.user_id WHERE d.user_id = $1 AND d.revoked_at IS NULL AND u.status = 'active' ORDER BY d.created_at, d.id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    /// Lists a stable, bounded administrator device page without returning account or credential IDs.
    pub async fn list_admin_devices(
        &self,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<AdminDeviceReadModel>, sqlx::Error> {
        sqlx::query_as::<_, AdminDeviceReadModelRow>(
            "SELECT d.device_type, d.device_display_name, \
             CASE WHEN d.revoked_at IS NOT NULL THEN 'revoked' WHEN EXISTS (SELECT 1 FROM sessions s WHERE s.device_id = d.id AND s.revoked_at IS NULL AND s.access_expires_at > now()) THEN 'active' ELSE 'inactive' END AS status, \
             to_char(d.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at, \
             to_char(GREATEST(d.last_seen_at, (SELECT max(s.last_seen_at) FROM sessions s WHERE s.device_id = d.id)) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS last_seen_at \
             FROM devices d ORDER BY d.created_at, d.device_display_name, d.id LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    /// Renames one active device owned by the authenticated account and records a safe audit class.
    pub async fn rename_owned_device(
        &self,
        user_id: Uuid,
        device_id: Uuid,
        name: &str,
    ) -> Result<bool, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE devices SET device_display_name = $3 WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL \
             AND EXISTS (SELECT 1 FROM users WHERE id = $2 AND status = 'active')",
        )
        .bind(device_id)
        .bind(user_id)
        .bind(name)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        audit(
            &mut transaction,
            Some(user_id),
            Some(device_id),
            "device_renamed",
        )
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// Revokes the caller's browser session and records a safe logout audit event.
    pub async fn logout_browser_session(
        &self,
        user_id: Uuid,
        session_token_hash: &SecretHash,
        csrf_hash: &SecretHash,
    ) -> Result<bool, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE browser_sessions SET revoked_at = now() WHERE user_id = $1 AND session_token_hash = $2 AND csrf_token_hash = $3 \
             AND revoked_at IS NULL AND expires_at > now()",
        )
        .bind(user_id)
        .bind(session_token_hash.as_bytes())
        .bind(csrf_hash.as_bytes())
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        audit(&mut transaction, Some(user_id), None, "browser_logout").await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// Confirms a live browser session with a recent passkey assertion for one account.
    pub async fn browser_session_is_fresh_for_user(
        &self,
        user_id: Uuid,
        session_token_hash: &SecretHash,
        csrf_hash: &SecretHash,
    ) -> Result<bool, sqlx::Error> {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM browser_sessions b JOIN users u ON u.id = b.user_id \
             WHERE b.user_id = $1 AND b.session_token_hash = $2 AND b.csrf_token_hash = $3 \
             AND b.revoked_at IS NULL AND b.expires_at > now() \
             AND b.passkey_reauthenticated_at > now() - interval '2 minutes' AND u.status = 'active')",
        )
        .bind(user_id)
        .bind(session_token_hash.as_bytes())
        .bind(csrf_hash.as_bytes())
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }
}
