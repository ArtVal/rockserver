//! Private PostgreSQL account-store operations grouped by one responsibility.

use super::rows::*;
use super::*;

impl PostgresAccountStore {
    /// Stores a verified passkey public key and rejects duplicate authenticator credentials.
    pub async fn create_passkey_credential(
        &self,
        credential_id: Uuid,
        user_id: Uuid,
        credential_bytes: &[u8],
        public_key: &[u8],
        sign_count: i64,
        transports: &[String],
    ) -> Result<bool, sqlx::Error> {
        let inserted = sqlx::query(
            "INSERT INTO passkey_credentials (id, user_id, credential_id, public_key, sign_count, transports) \
             SELECT $1, id, $3, $4, $5, $6 FROM users WHERE id = $2 AND status = 'active'",
        )
        .bind(credential_id)
        .bind(user_id)
        .bind(credential_bytes)
        .bind(public_key)
        .bind(sign_count)
        .bind(transports)
        .execute(&self.pool)
        .await?;
        Ok(inserted.rows_affected() == 1)
    }

    /// Atomically creates a new account, passkey, browser session, and consumed registration challenge.
    pub async fn complete_passkey_registration(
        &self,
        registration: NewPasskeyRegistration<'_>,
    ) -> Result<PasskeyRegistrationOutcome, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let consumed = sqlx::query(
            "UPDATE webauthn_challenges SET consumed_at = now() \
             WHERE id = $1 AND challenge_hash = $2 AND ceremony = 'registration' \
             AND origin = $3 AND rp_id = $4 AND user_id IS NULL \
             AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > now()",
        )
        .bind(registration.challenge_id)
        .bind(registration.challenge_hash.as_bytes())
        .bind(crate::auth::webauthn::ORIGIN)
        .bind(crate::auth::webauthn::RP_ID)
        .execute(&mut *transaction)
        .await?;
        if consumed.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(PasskeyRegistrationOutcome::ChallengeRejected);
        }

        let user_id = registration.user_id;
        sqlx::query("INSERT INTO users (id, account_display_name) VALUES ($1, $2)")
            .bind(user_id)
            .bind(registration.account_display_name)
            .execute(&mut *transaction)
            .await?;
        let credential_insert = sqlx::query(
            "INSERT INTO passkey_credentials (id, user_id, credential_id, public_key, sign_count, transports) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(registration.credential_id)
        .bind(user_id)
        .bind(registration.credential_bytes)
        .bind(registration.public_key)
        .bind(registration.sign_count)
        .bind(registration.transports)
        .execute(&mut *transaction)
        .await;
        if let Err(error) = credential_insert {
            if matches!(
                &error,
                sqlx::Error::Database(database_error)
                    if database_error.code().as_deref() == Some("23505")
            ) {
                transaction.rollback().await?;
                return Ok(PasskeyRegistrationOutcome::CredentialAlreadyRegistered);
            }
            return Err(error);
        }
        sqlx::query(
            "INSERT INTO browser_sessions (id, user_id, session_token_hash, csrf_token_hash, passkey_reauthenticated_at, expires_at, absolute_expires_at) \
             VALUES ($1, $2, $3, $4, now(), now() + ($5 * interval '1 minute'), now() + ($6 * interval '1 minute'))",
        )
        .bind(registration.browser_session.session_id)
        .bind(user_id)
        .bind(registration.browser_session.session_token_hash.as_bytes())
        .bind(registration.browser_session.csrf_hash.as_bytes())
        .bind(crate::auth::BROWSER_SESSION_IDLE_MINUTES)
        .bind(crate::auth::BROWSER_SESSION_ABSOLUTE_MINUTES)
        .execute(&mut *transaction)
        .await?;
        audit(&mut transaction, Some(user_id), None, "account_registered").await?;
        transaction.commit().await?;
        Ok(PasskeyRegistrationOutcome::Created)
    }

    /// Returns active passkey credential identifiers for a live account.
    pub async fn list_passkey_credential_ids(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<CredentialId>, sqlx::Error> {
        let rows = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT credential_id FROM passkey_credentials WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(CredentialId).collect())
    }

    /// Loads the public material needed for cryptographic assertion verification.
    pub async fn find_passkey_credential(
        &self,
        credential_id: &[u8],
    ) -> Result<Option<PasskeyCredential>, sqlx::Error> {
        let row = sqlx::query_as::<_, PasskeyCredentialRow>(
            "SELECT credential_id, public_key, sign_count, transports FROM passkey_credentials WHERE credential_id = $1 AND revoked_at IS NULL",
        )
        .bind(credential_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| PasskeyCredential {
            id: CredentialId(row.credential_id),
            public_key_cose: CosePublicKey(row.public_key),
            counter: u32::try_from(row.sign_count).unwrap_or(u32::MAX),
            transports: row.transports,
            aaguid: [0; 16],
        }))
    }

    /// Loads a credential only when it belongs to the account named by the ceremony.
    pub async fn find_passkey_credential_for_user(
        &self,
        user_id: Uuid,
        credential_id: &[u8],
    ) -> Result<Option<PasskeyCredential>, sqlx::Error> {
        let row = sqlx::query_as::<_, PasskeyCredentialRow>(
            "SELECT credential_id, public_key, sign_count, transports FROM passkey_credentials WHERE user_id = $1 AND credential_id = $2 AND revoked_at IS NULL",
        )
        .bind(user_id)
        .bind(credential_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| PasskeyCredential {
            id: CredentialId(row.credential_id),
            public_key_cose: CosePublicKey(row.public_key),
            counter: u32::try_from(row.sign_count).unwrap_or(u32::MAX),
            transports: row.transports,
            aaguid: [0; 16],
        }))
    }

    /// Atomically advances a passkey counter after the caller validates the WebAuthn signature.
    pub async fn advance_passkey_sign_count(
        &self,
        credential_bytes: &[u8],
        new_sign_count: i64,
    ) -> Result<bool, sqlx::Error> {
        let updated = sqlx::query(
            "UPDATE passkey_credentials SET sign_count = $2, last_used_at = now() \
             WHERE credential_id = $1 AND revoked_at IS NULL AND $2 >= sign_count \
             AND NOT (sign_count > 0 AND $2 = 0)",
        )
        .bind(credential_bytes)
        .bind(new_sign_count)
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() == 1)
    }

    /// Persists a single-use WebAuthn challenge with its expected origin and RP ID.
    pub async fn create_webauthn_challenge(
        &self,
        challenge: NewWebAuthnChallenge<'_>,
    ) -> Result<bool, sqlx::Error> {
        let ceremony = match challenge.ceremony {
            WebAuthnCeremony::Registration => "registration",
            WebAuthnCeremony::Authentication => "authentication",
        };
        let inserted = sqlx::query(
            "INSERT INTO webauthn_challenges (id, challenge_hash, state_blob, ceremony, rp_id, origin, expires_at, user_id, browser_session_id, pairing_request_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7::timestamptz, $8, $9, $10)",
        )
        .bind(challenge.challenge_id)
        .bind(challenge.challenge_hash.as_bytes())
        .bind(challenge.state_blob)
        .bind(ceremony)
        .bind(challenge.rp_id)
        .bind(challenge.origin)
        .bind(challenge.expires_at_rfc3339)
        .bind(challenge.user_id)
        .bind(challenge.browser_session_id)
        .bind(challenge.pairing_request_id)
        .execute(&self.pool)
        .await?;
        Ok(inserted.rows_affected() == 1)
    }

    /// Creates a WebAuthn challenge with a database-clock expiry and opaque serialized state.
    pub async fn create_webauthn_challenge_for_minutes(
        &self,
        challenge: NewWebAuthnChallenge<'_>,
        lifetime_minutes: i32,
    ) -> Result<bool, sqlx::Error> {
        let ceremony = match challenge.ceremony {
            WebAuthnCeremony::Registration => "registration",
            WebAuthnCeremony::Authentication => "authentication",
        };
        let inserted = sqlx::query(
            "INSERT INTO webauthn_challenges (id, challenge_hash, state_blob, ceremony, rp_id, origin, expires_at, user_id, browser_session_id, pairing_request_id) \
             VALUES ($1, $2, $3, $4, $5, $6, now() + ($7 * interval '1 minute'), $8, $9, $10)",
        )
        .bind(challenge.challenge_id)
        .bind(challenge.challenge_hash.as_bytes())
        .bind(challenge.state_blob)
        .bind(ceremony)
        .bind(challenge.rp_id)
        .bind(challenge.origin)
        .bind(lifetime_minutes)
        .bind(challenge.user_id)
        .bind(challenge.browser_session_id)
        .bind(challenge.pairing_request_id)
        .execute(&self.pool)
        .await?;
        Ok(inserted.rows_affected() == 1)
    }

    /// Loads an unconsumed WebAuthn state blob while the database enforces its expiry window.
    pub async fn load_webauthn_challenge_state(
        &self,
        challenge_id: Uuid,
    ) -> Result<Option<Vec<u8>>, sqlx::Error> {
        sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT state_blob FROM webauthn_challenges WHERE id = $1 AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > now()",
        )
        .bind(challenge_id)
        .fetch_optional(&self.pool)
        .await
    }

    /// Consumes a challenge exactly once when all server-bound context matches.
    pub async fn consume_webauthn_challenge(
        &self,
        challenge_id: Uuid,
        challenge_hash: &SecretHash,
        ceremony: WebAuthnCeremony,
        origin: &str,
        rp_id: &str,
    ) -> Result<bool, sqlx::Error> {
        let ceremony = match ceremony {
            WebAuthnCeremony::Registration => "registration",
            WebAuthnCeremony::Authentication => "authentication",
        };
        let updated = sqlx::query(
            "UPDATE webauthn_challenges SET consumed_at = now() \
             WHERE id = $1 AND challenge_hash = $2 AND ceremony = $3 AND origin = $4 AND rp_id = $5 \
             AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > now()",
        )
        .bind(challenge_id)
        .bind(challenge_hash.as_bytes())
        .bind(ceremony)
        .bind(origin)
        .bind(rp_id)
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() == 1)
    }
}
