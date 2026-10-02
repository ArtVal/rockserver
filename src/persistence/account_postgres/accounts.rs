//! Private PostgreSQL account-store operations grouped by one responsibility.

use super::rows::*;
use super::*;

impl PostgresAccountStore {
    /// Creates one active passkey-only account without contact or password data.
    pub async fn create_user(&self, user_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO users (id) VALUES ($1)")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Creates an account with the explicit user-facing label used for its passkey and browser flow.
    pub async fn create_user_with_display_name(
        &self,
        user_id: Uuid,
        account_display_name: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO users (id, account_display_name) VALUES ($1, $2)")
            .bind(user_id)
            .bind(account_display_name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Creates a fixture device with a deterministic placeholder credential hash.
    ///
    /// Production devices are created only by pairing, which generates a random device secret.
    pub async fn create_device(&self, device: &OwnedDevice) -> Result<bool, sqlx::Error> {
        let mut test_only_secret_hash = [0_u8; 32];
        test_only_secret_hash[..16].copy_from_slice(device.id.as_bytes());
        test_only_secret_hash[16..].copy_from_slice(device.id.as_bytes());
        let inserted = sqlx::query(
            "INSERT INTO devices (id, user_id, device_display_name, device_type, device_secret_hash) \
             SELECT $1, id, $3, $4, $5 FROM users WHERE id = $2 AND status = 'active'",
        )
        .bind(device.id)
        .bind(device.user_id)
        .bind(&device.device_display_name)
        .bind(&device.device_type)
        .bind(test_only_secret_hash)
        .execute(&self.pool)
        .await?;
        Ok(inserted.rows_affected() == 1)
    }

    /// Returns a device only if the caller owns it and neither record has been revoked/deleted.
    pub async fn find_owned_device(
        &self,
        user_id: Uuid,
        device_id: Uuid,
    ) -> Result<Option<OwnedDevice>, sqlx::Error> {
        sqlx::query_as::<_, DeviceRow>(
            "SELECT d.id, d.user_id, d.device_display_name, d.device_type, to_char(d.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at, to_char(d.last_seen_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS last_seen_at FROM devices d \
             JOIN users u ON u.id = d.user_id \
             WHERE d.id = $1 AND d.user_id = $2 AND d.revoked_at IS NULL AND u.status = 'active'",
        )
        .bind(device_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(Into::into))
    }

    /// Resolves a non-expired access token hash to its active account/session owner.
    pub async fn find_active_session_by_access_hash(
        &self,
        access_hash: &SecretHash,
    ) -> Result<Option<ActiveSession>, sqlx::Error> {
        sqlx::query_as::<_, ActiveSessionRow>(
            "SELECT s.id AS session_id, s.user_id, s.device_id FROM sessions s \
             JOIN users u ON u.id = s.user_id JOIN devices d ON d.id = s.device_id \
             WHERE s.access_token_hash = $1 AND s.revoked_at IS NULL AND s.access_expires_at > now() \
             AND u.status = 'active' AND d.revoked_at IS NULL AND d.user_id = s.user_id",
        )
        .bind(access_hash.as_bytes())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(Into::into))
    }

    /// Returns the account and current-device labels only for an already authenticated native owner.
    pub async fn account_projection(
        &self,
        user_id: Uuid,
        device_id: Uuid,
    ) -> Result<Option<AccountProjection>, sqlx::Error> {
        sqlx::query_as::<_, AccountProjectionRow>(
            "SELECT u.account_display_name, to_char(u.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at, d.device_display_name, d.device_type, \
             to_char(d.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS device_created_at, to_char(d.last_seen_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS last_seen_at \
             FROM users u JOIN devices d ON d.user_id = u.id \
             WHERE u.id = $1 AND d.id = $2 AND u.status = 'active' AND d.revoked_at IS NULL",
        )
        .bind(user_id)
        .bind(device_id)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(Into::into))
    }

    /// Resolves a browser account label from an unexpired opaque cookie hash without exposing its owner ID.
    pub async fn browser_account_display_name(
        &self,
        session_token_hash: &SecretHash,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT u.account_display_name FROM browser_sessions b JOIN users u ON u.id = b.user_id \
             WHERE b.session_token_hash = $1 AND b.revoked_at IS NULL AND b.expires_at > now() AND u.status = 'active'",
        )
        .bind(session_token_hash.as_bytes())
        .fetch_optional(&self.pool)
        .await
    }

    /// Rotates the tab-local CSRF proof and returns the active account display name.
    pub async fn rotate_browser_csrf(
        &self,
        session_token_hash: &SecretHash,
        csrf_token_hash: &SecretHash,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar(
            "UPDATE browser_sessions b SET csrf_token_hash = $2 \
             FROM users u WHERE b.user_id = u.id AND b.session_token_hash = $1 \
             AND b.revoked_at IS NULL AND b.expires_at > now() AND u.status = 'active' \
             RETURNING u.account_display_name",
        )
        .bind(session_token_hash.as_bytes())
        .bind(csrf_token_hash.as_bytes())
        .fetch_optional(&self.pool)
        .await
    }

    /// Returns an active account's display name for a server-derived owner ID.
    pub async fn account_display_name(&self, user_id: Uuid) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT account_display_name FROM users WHERE id = $1 AND status = 'active'",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
    }

    /// Lists only active devices owned by the requested account.
    pub async fn list_owned_devices(&self, user_id: Uuid) -> Result<Vec<OwnedDevice>, sqlx::Error> {
        sqlx::query_as::<_, DeviceRow>(
            "SELECT d.id, d.user_id, d.device_display_name, d.device_type, to_char(d.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at, to_char(d.last_seen_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS last_seen_at FROM devices d \
             JOIN users u ON u.id = d.user_id WHERE d.user_id = $1 AND d.revoked_at IS NULL \
             AND u.status = 'active' ORDER BY d.created_at, d.id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    /// Revokes one device and all of its native access sessions.
    pub async fn revoke_owned_device(
        &self,
        user_id: Uuid,
        device_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE devices SET revoked_at = now() WHERE id = $1 AND user_id = $2 \
             AND revoked_at IS NULL AND EXISTS (SELECT 1 FROM users WHERE id = $2 AND status = 'active')",
        )
        .bind(device_id)
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        sqlx::query("UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND device_id = $2 AND revoked_at IS NULL")
            .bind(user_id).bind(device_id).execute(&mut *transaction).await?;
        audit(
            &mut transaction,
            Some(user_id),
            Some(device_id),
            "device_revoked",
        )
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// Persists one short-lived native access session.
    pub async fn create_session(&self, session: NewSession<'_>) -> Result<bool, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let inserted = sqlx::query(
            "INSERT INTO sessions (id, user_id, device_id, access_token_hash, access_expires_at) \
             SELECT $1, $2, d.id, $4, $5::timestamptz FROM devices d JOIN users u ON u.id = d.user_id \
             WHERE d.id = $3 AND d.user_id = $2 AND d.revoked_at IS NULL AND u.status = 'active'",
        )
        .bind(session.session_id).bind(session.user_id).bind(session.device_id).bind(session.access_hash.as_bytes())
        .bind(session.access_expires_at_rfc3339).execute(&mut *transaction).await?;
        if inserted.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        transaction.commit().await?;
        Ok(true)
    }

    /// Replaces a device's access session after validating its persistent opaque credential.
    pub async fn issue_device_session(
        &self,
        device_id: Uuid,
        device_secret_hash: &SecretHash,
        session_id: Uuid,
        access_hash: &SecretHash,
    ) -> Result<Option<String>, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let owner = sqlx::query_scalar::<_, Uuid>(
            "SELECT d.user_id FROM devices d JOIN users u ON u.id = d.user_id \
             WHERE d.id = $1 AND d.device_secret_hash = $2 AND d.revoked_at IS NULL \
             AND u.status = 'active' FOR UPDATE",
        )
        .bind(device_id)
        .bind(device_secret_hash.as_bytes())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(user_id) = owner else {
            transaction.rollback().await?;
            return Ok(None);
        };
        sqlx::query(
            "UPDATE sessions SET revoked_at = now() WHERE device_id = $1 AND revoked_at IS NULL",
        )
        .bind(device_id)
        .execute(&mut *transaction)
        .await?;
        let access_expires_at = sqlx::query_scalar::<_, String>(
            "INSERT INTO sessions (id, user_id, device_id, access_token_hash, access_expires_at) \
             VALUES ($1, $2, $3, $4, now() + interval '15 minutes') \
             RETURNING to_char(access_expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",
        )
        .bind(session_id)
        .bind(user_id)
        .bind(device_id)
        .bind(access_hash.as_bytes())
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query("UPDATE devices SET last_seen_at = now() WHERE id = $1")
            .bind(device_id)
            .execute(&mut *transaction)
            .await?;
        audit(
            &mut transaction,
            Some(user_id),
            Some(device_id),
            "device_session_issued",
        )
        .await?;
        transaction.commit().await?;
        Ok(Some(access_expires_at))
    }

    /// Creates a first-party browser session only for an active account.
    pub async fn create_browser_session(
        &self,
        session: NewBrowserSession<'_>,
    ) -> Result<bool, sqlx::Error> {
        let inserted = sqlx::query(
            "INSERT INTO browser_sessions (id, user_id, session_token_hash, csrf_token_hash, passkey_reauthenticated_at, expires_at, absolute_expires_at) \
             SELECT $1, id, $3, $4, $5::timestamptz, $6::timestamptz, $7::timestamptz FROM users WHERE id = $2 AND status = 'active'",
        )
        .bind(session.session_id)
        .bind(session.user_id)
        .bind(session.session_token_hash.as_bytes())
        .bind(session.csrf_hash.as_bytes())
        .bind(session.passkey_reauthenticated_at_rfc3339)
        .bind(session.expires_at_rfc3339)
        .bind(session.absolute_expires_at_rfc3339)
        .execute(&self.pool)
        .await?;
        Ok(inserted.rows_affected() == 1)
    }
}
