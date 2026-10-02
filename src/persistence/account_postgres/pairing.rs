//! Private PostgreSQL account-store operations grouped by one responsibility.

use super::rows::*;
use super::*;

impl PostgresAccountStore {
    /// Creates an unapproved desktop pairing request with three independent hashed proofs.
    pub async fn create_pairing_request(
        &self,
        request: NewPairingRequest<'_>,
    ) -> Result<bool, sqlx::Error> {
        let inserted = sqlx::query(
            "INSERT INTO pairing_requests (id, desktop_token_hash, approval_secret_hash, short_code_hash, verification_phrase, device_display_name, device_type, app_version, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::timestamptz)",
        )
        .bind(request.request_id)
        .bind(request.desktop_token_hash.as_bytes())
        .bind(request.approval_secret_hash.as_bytes())
        .bind(request.short_code_hash.as_bytes())
        .bind(request.verification_phrase)
        .bind(request.device_display_name)
        .bind(request.device_type)
        .bind(request.app_version)
        .bind(request.expires_at_rfc3339)
        .execute(&self.pool)
        .await?;
        Ok(inserted.rows_affected() == 1)
    }

    /// Creates a pairing request with a database-clock expiry, avoiding an application-clock trust boundary.
    pub async fn create_pairing_request_for_minutes(
        &self,
        request: NewPairingRequest<'_>,
        lifetime_minutes: i32,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar(
            "INSERT INTO pairing_requests (id, desktop_token_hash, approval_secret_hash, short_code_hash, verification_phrase, device_display_name, device_type, app_version, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now() + ($9 * interval '1 minute')) RETURNING to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",
        )
        .bind(request.request_id)
        .bind(request.desktop_token_hash.as_bytes())
        .bind(request.approval_secret_hash.as_bytes())
        .bind(request.short_code_hash.as_bytes())
        .bind(request.verification_phrase)
        .bind(request.device_display_name)
        .bind(request.device_type)
        .bind(request.app_version)
        .bind(lifetime_minutes)
        .fetch_optional(&self.pool)
        .await
    }

    /// Looks up an unexpired request by short-code hash without returning any secret proof.
    pub async fn lookup_pairing_request(
        &self,
        short_code_hash: &SecretHash,
    ) -> Result<Option<PairingPreview>, sqlx::Error> {
        sqlx::query_as::<_, PairingPreviewRow>(
            "SELECT id, device_display_name, device_type, app_version, verification_phrase, to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS expires_at FROM pairing_requests \
             WHERE short_code_hash = $1 AND approved_at IS NULL AND consumed_at IS NULL \
             AND revoked_at IS NULL AND expires_at > now()",
        )
        .bind(short_code_hash.as_bytes())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(Into::into))
    }

    /// Approves a pending request from a live browser session owned by the account.
    pub async fn approve_pairing_request(
        &self,
        request_id: Uuid,
        user_id: Uuid,
        browser_session_id: Uuid,
        approval_secret_hash: &SecretHash,
        verification_phrase: &str,
    ) -> Result<bool, sqlx::Error> {
        let updated = sqlx::query(
            "UPDATE pairing_requests p SET approved_by_user_id = $2, approved_at = now() \
             FROM browser_sessions b JOIN users u ON u.id = b.user_id \
             WHERE p.id = $1 AND p.approval_secret_hash = $4 AND p.verification_phrase = $5 AND p.approved_at IS NULL AND p.consumed_at IS NULL AND p.revoked_at IS NULL AND p.expires_at > now() \
             AND b.id = $3 AND b.user_id = $2 AND b.revoked_at IS NULL AND b.expires_at > now() \
             AND u.status = 'active'",
        )
        .bind(request_id)
        .bind(user_id)
        .bind(browser_session_id)
        .bind(approval_secret_hash.as_bytes())
        .bind(verification_phrase)
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() == 1)
    }

    /// Approves a request only with the opaque browser cookie and its CSRF proof.
    pub async fn approve_pairing_request_with_browser_proof(
        &self,
        request_id: Uuid,
        approval_secret_hash: &SecretHash,
        verification_phrase: &str,
        session_token_hash: &SecretHash,
        csrf_hash: &SecretHash,
    ) -> Result<bool, sqlx::Error> {
        let updated = sqlx::query(
            "UPDATE pairing_requests p SET approved_by_user_id = b.user_id, approved_at = now() \
             FROM browser_sessions b JOIN users u ON u.id = b.user_id \
             WHERE p.id = $1 AND p.approval_secret_hash = $2 AND p.verification_phrase = $3 \
             AND p.approved_at IS NULL AND p.consumed_at IS NULL AND p.revoked_at IS NULL AND p.expires_at > now() \
             AND b.session_token_hash = $4 AND b.csrf_token_hash = $5 AND b.revoked_at IS NULL AND b.expires_at > now() \
             AND u.status = 'active'",
        )
        .bind(request_id)
        .bind(approval_secret_hash.as_bytes())
        .bind(verification_phrase)
        .bind(session_token_hash.as_bytes())
        .bind(csrf_hash.as_bytes())
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() == 1)
    }

    /// Atomically consumes an approved request and creates an owner-scoped device/session pair.
    pub async fn complete_pairing(
        &self,
        request_id: Uuid,
        desktop_token_hash: &SecretHash,
        session: NewPairingSession<'_>,
    ) -> Result<PairingCompletionOutcome, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let request = sqlx::query_as::<_, PairingRow>(
            "SELECT approved_by_user_id, device_display_name, device_type, app_version, \
             approved_at IS NOT NULL AS approved, consumed_at IS NOT NULL AS consumed, \
             revoked_at IS NOT NULL AS revoked, expires_at <= now() AS expired \
             FROM pairing_requests WHERE id = $1 AND desktop_token_hash = $2 FOR UPDATE",
        )
        .bind(request_id)
        .bind(desktop_token_hash.as_bytes())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(request) = request else {
            transaction.rollback().await?;
            return Ok(PairingCompletionOutcome::InvalidProof);
        };
        if request.revoked || request.consumed || request.expired {
            transaction.rollback().await?;
            return Ok(PairingCompletionOutcome::NoLongerAvailable);
        }
        if !request.approved {
            transaction.rollback().await?;
            return Ok(PairingCompletionOutcome::Pending);
        }
        let Some(user_id) = request.approved_by_user_id else {
            transaction.rollback().await?;
            return Ok(PairingCompletionOutcome::NoLongerAvailable);
        };
        // Serialize completions for one account so concurrent requests cannot bypass the device cap.
        let account_display_name = sqlx::query_scalar::<_, String>(
            "SELECT account_display_name FROM users WHERE id = $1 AND status = 'active' FOR UPDATE",
        )
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(account_display_name) = account_display_name else {
            transaction.rollback().await?;
            return Ok(PairingCompletionOutcome::NoLongerAvailable);
        };
        let inserted = sqlx::query(
            "INSERT INTO devices (id, user_id, device_display_name, device_type, app_version, device_secret_hash) \
             SELECT $1, $2, $3, $4, $5, $6 WHERE (SELECT COUNT(*) FROM devices WHERE user_id = $2 AND revoked_at IS NULL) < $7",
        )
        .bind(session.device_id)
        .bind(user_id)
        .bind(&request.device_display_name)
        .bind(&request.device_type)
        .bind(request.app_version)
        .bind(session.device_secret_hash.as_bytes())
        .bind(i64::from(MAX_ACCOUNT_DEVICES))
        .execute(&mut *transaction)
        .await?;
        if inserted.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(PairingCompletionOutcome::DeviceLimit);
        }
        let access_expires_at = sqlx::query_scalar::<_, String>(
            "INSERT INTO sessions (id, user_id, device_id, access_token_hash, access_expires_at) VALUES ($1, $2, $3, $4, CASE WHEN $5 = 'db:15m' THEN now() + interval '15 minutes' ELSE $5::timestamptz END) \
             RETURNING to_char(access_expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",
        )
        .bind(session.session_id)
        .bind(user_id)
        .bind(session.device_id)
        .bind(session.access_hash.as_bytes())
        .bind(session.access_expires_at_rfc3339)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query("UPDATE pairing_requests SET consumed_at = now() WHERE id = $1")
            .bind(request_id)
            .execute(&mut *transaction)
            .await?;
        audit(
            &mut transaction,
            Some(user_id),
            Some(session.device_id),
            "pairing_completed",
        )
        .await?;
        transaction.commit().await?;
        Ok(PairingCompletionOutcome::Completed(PairingCompletion {
            user_id,
            device_id: session.device_id,
            session_id: session.session_id,
            access_expires_at,
            account_display_name,
            device_display_name: request.device_display_name,
            device_type: request.device_type,
        }))
    }
}
