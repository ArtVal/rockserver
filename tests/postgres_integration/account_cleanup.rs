//! Integration tests for account cleanup preview, operator revocation, and cascade deletion.

use std::env;

use rockserver::account_cleanup::CleanupError;
use rockserver::auth::{NewBrowserSession, NewSession, OwnedDevice, SecretHash};
use rockserver::persistence::PostgresAccountStore;
use uuid::Uuid;

/// Verifies preview-only inspection, exact-target guards, operator audit, and account cascades.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_account_cleanup_is_preview_first_and_cascade_safe() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAccountStore::connect(&database_url)
        .await
        .expect("account migrations must succeed");
    let inspection = sqlx::PgPool::connect(&database_url).await.unwrap();
    let account_id = Uuid::new_v4();
    store
        .create_user_with_display_name(account_id, "staging test account")
        .await
        .unwrap();
    let first_credential_id = Uuid::new_v4();
    let second_credential_id = Uuid::new_v4();
    for credential_id in [first_credential_id, second_credential_id] {
        assert!(
            store
                .create_passkey_credential(
                    credential_id,
                    account_id,
                    credential_id.as_bytes(),
                    b"public-key",
                    0,
                    &[],
                )
                .await
                .unwrap()
        );
    }
    let device_id = Uuid::new_v4();
    assert!(
        store
            .create_device(&OwnedDevice {
                id: device_id,
                user_id: account_id,
                device_display_name: "staging device".to_owned(),
                device_type: "rockcast_windows".to_owned(),
                created_at: "unused".to_owned(),
                last_seen_at: None,
            })
            .await
            .unwrap()
    );
    let session_id = Uuid::new_v4();
    assert!(
        store
            .create_session(NewSession {
                session_id,
                user_id: account_id,
                device_id,
                access_hash: &SecretHash::new([41; 32]),
                access_expires_at_rfc3339: "2035-01-01T00:00:00Z",
            })
            .await
            .unwrap()
    );
    let browser_session_id = Uuid::new_v4();
    assert!(
        store
            .create_browser_session(NewBrowserSession {
                session_id: browser_session_id,
                user_id: account_id,
                session_token_hash: &SecretHash::new([43; 32]),
                csrf_hash: &SecretHash::new([44; 32]),
                passkey_reauthenticated_at_rfc3339: "2035-01-01T00:00:00Z",
                expires_at_rfc3339: "2035-02-01T00:00:00Z",
                absolute_expires_at_rfc3339: "2035-08-01T00:00:00Z",
            })
            .await
            .unwrap()
    );

    let preview = store.account_cleanup_preview().await.unwrap();
    let preview_account = preview
        .accounts
        .iter()
        .find(|account| account.account_id == account_id)
        .expect("preview must include the account");
    assert_eq!(preview_account.status, "active");
    assert_eq!(preview_account.candidate_status, "review_required");
    assert_eq!(preview_account.dependency_counts.passkeys, 2);
    assert_eq!(preview_account.dependency_counts.devices, 1);
    assert_eq!(preview_account.dependency_counts.sessions, 1);
    assert_eq!(preview_account.dependency_counts.browser_sessions, 1);
    assert!(
        preview_account
            .dependencies
            .iter()
            .all(|dependency| dependency.kind != "credential_material")
    );
    assert!(
        preview_account
            .dependencies
            .iter()
            .all(|dependency| !dependency.candidate_reason.is_empty())
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM users WHERE id = $1")
            .bind(account_id)
            .fetch_one(&inspection)
            .await
            .unwrap(),
        "active"
    );
    assert_eq!(
        store
            .revoke_credential_for_operator(first_credential_id, "REVOKE CREDENTIAL *")
            .await,
        Err(CleanupError::InvalidConfirmation)
    );

    let credential_revoke = store
        .revoke_credential_for_operator(
            first_credential_id,
            &rockserver::account_cleanup::confirmation_for(
                rockserver::account_cleanup::CleanupAction::RevokeCredential,
                first_credential_id,
            ),
        )
        .await
        .unwrap();
    assert_eq!(credential_revoke.status, "revoked");
    assert_eq!(credential_revoke.revoked.passkeys, 1);
    assert_eq!(
        store
            .revoke_credential_for_operator(
                second_credential_id,
                &rockserver::account_cleanup::confirmation_for(
                    rockserver::account_cleanup::CleanupAction::RevokeCredential,
                    second_credential_id,
                ),
            )
            .await,
        Err(CleanupError::LastWorkingPasskey)
    );
    let device_revoke = store
        .revoke_device_for_operator(
            device_id,
            &rockserver::account_cleanup::confirmation_for(
                rockserver::account_cleanup::CleanupAction::RevokeDevice,
                device_id,
            ),
        )
        .await
        .unwrap();
    assert_eq!(device_revoke.revoked.devices, 1);
    assert_eq!(device_revoke.revoked.sessions, 1);

    let protected_account_id = Uuid::new_v4();
    store.create_user(protected_account_id).await.unwrap();
    sqlx::query("INSERT INTO account_identities (id, user_id, kind, subject_hash, subject_ciphertext) VALUES ($1, $2, 'admin', $3, $4)")
        .bind(Uuid::new_v4())
        .bind(protected_account_id)
        .bind([45u8; 32].as_slice())
        .bind(b"protected")
        .execute(&inspection)
        .await
        .unwrap();
    assert_eq!(
        store
            .deactivate_account_for_operator(
                protected_account_id,
                &rockserver::account_cleanup::confirmation_for(
                    rockserver::account_cleanup::CleanupAction::DeactivateAccount,
                    protected_account_id,
                ),
            )
            .await,
        Err(CleanupError::ProtectedAccount)
    );

    let result = store
        .deactivate_account_for_operator(
            account_id,
            &rockserver::account_cleanup::confirmation_for(
                rockserver::account_cleanup::CleanupAction::DeactivateAccount,
                account_id,
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.status, "deactivated");
    // The counts report rows this deactivation flips: the device and its native session were
    // already revoked by revoke_device_for_operator above, so only the remaining passkey
    // flips here. The raw state check below proves the cascade leaves nothing active.
    assert_eq!(result.revoked.devices, 0);
    assert_eq!(result.revoked.sessions, 0);
    assert_eq!(result.revoked.passkeys, 1);
    let state = sqlx::query_as::<_, (String, bool, bool, bool)>(
        "SELECT u.status, EXISTS(SELECT 1 FROM devices d WHERE d.user_id = u.id AND d.revoked_at IS NULL), EXISTS(SELECT 1 FROM sessions s WHERE s.user_id = u.id AND s.revoked_at IS NULL), EXISTS(SELECT 1 FROM browser_sessions b WHERE b.user_id = u.id AND b.revoked_at IS NULL) FROM users u WHERE u.id = $1",
    )
    .bind(account_id)
    .fetch_one(&inspection)
    .await
    .unwrap();
    assert_eq!(state, ("deleted".to_owned(), false, false, false));
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT event_type FROM account_audit_events WHERE id = $1"
        )
        .bind(result.audit_event_id)
        .fetch_one(&inspection)
        .await
        .unwrap(),
        "operator_account_deactivated"
    );
    inspection.close().await;
    store.close().await;
}
