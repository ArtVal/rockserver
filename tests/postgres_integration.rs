//! Opt-in integration coverage against a real PostgreSQL test database.

use std::{collections::BTreeSet, env, sync::Arc};

use async_trait::async_trait;
use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use rockserver::{
    account_cleanup::CleanupError,
    admin::{
        AdminAuditFilter, AdminLoginAttempt, AdminLoginOutcome, AdminPasswordHash, AdminPrincipal,
        AdminPrincipalStatus, AdminRequestOutcome, AdminRequestRecord, AdminSecurityEvent,
        AdminSecurityEventType, AdminStore, AdminUsername, NewAdminPasswordCredential,
        NewAdminSession,
    },
    admin_bootstrap::{AdminBootstrapConfig, bootstrap_admin},
    auth::{
        NewBrowserSession, NewPairingRequest, NewPairingSession, NewSession, NewWebAuthnChallenge,
        OwnedDevice, PairingCompletionOutcome, SecretHash, WebAuthnCeremony,
    },
    catalog::{
        CatalogImportError, CatalogImportProvider, CatalogImporter, ImportLimits, ImportPage,
        ImportedStation, ImportedStream, PinnedSharedCatalog,
    },
    device_control::{
        CommandId, CommandReservation, CommandResult, CommandStatus, DeviceCommand,
        DeviceControlStore, DeviceId, DeviceManifest, DeviceStateSnapshot, DomainError,
        EntityState, StoreOutcome, Timestamp,
    },
    http::{HealthResponse, HealthStatus, router_with_repository, router_with_search_service},
    persistence::{
        EncryptedYandexHomeToken, OwnedCatalogReplacement, PostgresAccountStore,
        PostgresAdminStore, PostgresDeviceControlStore, PostgresEmbeddingStore,
        PostgresImportStore, PostgresPersonalDataStore, PostgresStationRepository,
    },
    personal_data::{
        CollectionChanges, FavouriteUpsert, HistoryUpsert, PersonalDataStore, RecordDelete,
        SyncRequest, Timestamp as PersonalTimestamp,
    },
    providers::radio_browser::SOURCE,
    search::{
        DeterministicQueryParser, Embedding, EmbeddingProvider, EmbeddingProviderError,
        EmbeddingStore, SearchAction, SearchConstraints, SearchQuery, SearchService,
        normalize_query,
    },
};
use serde_json::Value;
use sha2::Digest;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

/// Exercises migration 0021, active-owner filtering, manifest revisions, and tombstoned projections.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_device_control_manifest_is_owner_scoped_and_tombstoned() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let accounts = PostgresAccountStore::connect(&database_url).await.unwrap();
    let store = PostgresDeviceControlStore::connect(&database_url)
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    let device = Uuid::new_v4();
    accounts.create_user(owner).await.unwrap();
    accounts.create_user(foreign).await.unwrap();
    assert!(
        accounts
            .create_device(&OwnedDevice {
                id: device,
                user_id: owner,
                device_display_name: "control fixture".into(),
                device_type: "test".into(),
                created_at: "2026-09-02T00:00:00Z".into(),
                last_seen_at: None
            })
            .await
            .unwrap()
    );
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/device-control/v1/esp32-manifest-client.json"
    ))
    .unwrap();
    let mut manifest: DeviceManifest =
        serde_json::from_value(fixture["payload"]["manifest"].clone()).unwrap();
    assert_eq!(
        store
            .apply_manifest(owner, DeviceId(device), manifest.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .apply_manifest(owner, DeviceId(device), manifest.clone())
            .await
            .unwrap(),
        StoreOutcome::Replay
    );
    assert_eq!(
        store
            .apply_manifest(foreign, DeviceId(device), manifest.clone())
            .await
            .unwrap(),
        StoreOutcome::NotOwned
    );
    assert_eq!(
        store
            .list_entities(owner, DeviceId(device))
            .await
            .unwrap()
            .len(),
        manifest.entities.len()
    );
    let state_fixture: Value = serde_json::from_str(include_str!(
        "fixtures/device-control/v1/esp32-state-full-client.json"
    ))
    .unwrap();
    let state: DeviceStateSnapshot =
        serde_json::from_value(state_fixture["payload"]["snapshot"].clone()).unwrap();
    assert_eq!(
        store
            .store_device_state(owner, DeviceId(device), state.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .store_device_state(owner, DeviceId(device), state.clone())
            .await
            .unwrap(),
        StoreOutcome::Replay
    );
    let mut stale_state = state.clone();
    stale_state.state_revision -= 1;
    assert_eq!(
        store
            .store_device_state(owner, DeviceId(device), stale_state)
            .await
            .unwrap(),
        StoreOutcome::Stale
    );
    let mut conflicting_state = state.clone();
    conflicting_state.state.playback.as_mut().unwrap().status = "playing".into();
    assert_eq!(
        store
            .store_device_state(owner, DeviceId(device), conflicting_state)
            .await
            .unwrap(),
        StoreOutcome::Conflict
    );
    let mut gap_state = state.clone();
    gap_state.state_revision += 2;
    assert_eq!(
        store
            .store_device_state(owner, DeviceId(device), gap_state)
            .await
            .unwrap(),
        StoreOutcome::Resync
    );
    assert_eq!(
        store
            .load_device_state(owner, DeviceId(device))
            .await
            .unwrap()
            .unwrap()
            .state_revision,
        state.state_revision
    );
    let entity_fixture: Value = serde_json::from_str(include_str!(
        "fixtures/device-control/v1/esp32-temperature-state-client.json"
    ))
    .unwrap();
    let entity_state: EntityState =
        serde_json::from_value(entity_fixture["payload"]["state"].clone()).unwrap();
    assert_eq!(
        store
            .store_entity_state(owner, DeviceId(device), entity_state.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .store_entity_state(owner, DeviceId(device), entity_state.clone())
            .await
            .unwrap(),
        StoreOutcome::Replay
    );
    let mut stale_entity = entity_state.clone();
    stale_entity.entity_revision -= 1;
    assert_eq!(
        store
            .store_entity_state(owner, DeviceId(device), stale_entity)
            .await
            .unwrap(),
        StoreOutcome::Stale
    );
    assert!(
        store
            .load_entity_state(owner, DeviceId(device), &entity_state.entity_id)
            .await
            .unwrap()
            .is_some()
    );
    let command_fixture: Value = serde_json::from_str(include_str!(
        "fixtures/device-control/v1/display-sensor-grid-command-server.json"
    ))
    .unwrap();
    let mut command: DeviceCommand =
        serde_json::from_value(command_fixture["payload"].clone()).unwrap();
    command.command_id = CommandId(Uuid::new_v4());
    command.target.device_id = DeviceId(device);
    let reservation = CommandReservation {
        command: command.clone(),
        fingerprint: [7; 32],
        deadline_at: Timestamp::parse("2030-01-01T00:00:00Z").unwrap(),
    };
    assert_eq!(
        store
            .reserve_command(owner, DeviceId(device), reservation.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .reserve_command(owner, DeviceId(device), reservation.clone())
            .await
            .unwrap(),
        StoreOutcome::Replay
    );
    let mut conflicting = reservation.clone();
    conflicting.fingerprint = [8; 32];
    assert_eq!(
        store
            .reserve_command(owner, DeviceId(device), conflicting)
            .await
            .unwrap(),
        StoreOutcome::Conflict
    );
    let result = CommandResult {
        command_id: command.command_id,
        status: CommandStatus::Succeeded,
        completed_at: Timestamp::parse("2026-09-02T12:03:00Z").unwrap(),
        error: None,
    };
    assert_eq!(
        store
            .complete_command(owner, DeviceId(device), result.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .complete_command(owner, DeviceId(device), result)
            .await
            .unwrap(),
        StoreOutcome::Replay
    );
    let conflicting_result = CommandResult {
        command_id: command.command_id,
        status: CommandStatus::Failed,
        completed_at: Timestamp::parse("2026-09-02T12:03:01Z").unwrap(),
        error: Some(DomainError {
            code: "failed".into(),
            message: "safe test error".into(),
            request_id: "postgres-test".into(),
            details: Default::default(),
        }),
    };
    assert_eq!(
        store
            .complete_command(owner, DeviceId(device), conflicting_result)
            .await
            .unwrap(),
        StoreOutcome::Conflict
    );
    assert!(
        store
            .load_command(owner, DeviceId(device), command.command_id)
            .await
            .unwrap()
            .unwrap()
            .result
            .is_some()
    );
    let pool = PgPool::connect(&database_url).await.unwrap();
    assert!(sqlx::query("INSERT INTO device_control_manifests(device_id,revision,payload) VALUES($1,0,'{}'::jsonb)")
        .bind(Uuid::new_v4()).execute(&pool).await.is_err());
    sqlx::query("UPDATE device_control_commands SET completed_at=now()-interval '25 hours' WHERE command_id=$1")
        .bind(command.command_id.0).execute(&pool).await.unwrap();
    pool.close().await;
    assert_eq!(store.prune_commands(1).await.unwrap(), 1);
    assert!(
        store
            .load_command(owner, DeviceId(device), command.command_id)
            .await
            .unwrap()
            .is_none()
    );
    let original_entities = manifest.entities.clone();
    manifest.manifest_revision += 1;
    manifest.entities.clear();
    assert_eq!(
        store
            .apply_manifest(owner, DeviceId(device), manifest.clone())
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert!(
        store
            .list_entities(owner, DeviceId(device))
            .await
            .unwrap()
            .is_empty()
    );
    manifest.manifest_revision += 1;
    manifest.entities = original_entities;
    assert_eq!(
        store
            .apply_manifest(owner, DeviceId(device), manifest)
            .await
            .unwrap(),
        StoreOutcome::Accepted
    );
    assert_eq!(
        store
            .list_entities(owner, DeviceId(device))
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(accounts.revoke_owned_device(owner, device).await.unwrap());
    assert!(
        store
            .load_manifest(owner, DeviceId(device))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .store_device_state(owner, DeviceId(device), state)
            .await
            .unwrap(),
        StoreOutcome::NotOwned
    );
    store.close().await;
    accounts.close().await;
}

/// Covers administrator-only migrations and hashed credential/session persistence against PostgreSQL.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_admin_identity_foundation_rejects_invalid_hash_state() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAdminStore::connect(&database_url)
        .await
        .expect("admin migrations must succeed");
    let principal_id = Uuid::new_v4();
    store
        .create_principal(AdminPrincipal {
            id: principal_id,
            status: AdminPrincipalStatus::Active,
        })
        .await
        .unwrap();
    store
        .create_password_credential(NewAdminPasswordCredential {
            id: Uuid::new_v4(),
            principal_id,
            password_hash: AdminPasswordHash::parse(
                "$argon2id$v=19$m=65536,t=3,p=4$salt$hash".to_owned(),
            )
            .unwrap(),
        })
        .await
        .unwrap();
    assert!(
        store
            .active_password_credential(principal_id)
            .await
            .unwrap()
            .is_some()
    );
    let mut token_hash_bytes = [0_u8; 32];
    token_hash_bytes[..16].copy_from_slice(principal_id.as_bytes());
    token_hash_bytes[16..].copy_from_slice(principal_id.as_bytes());
    let token_hash = SecretHash::new(token_hash_bytes);
    store
        .create_session(NewAdminSession {
            id: Uuid::new_v4(),
            principal_id,
            token_hash: token_hash.clone(),
            ttl_seconds: 60,
        })
        .await
        .unwrap();
    assert!(
        store
            .find_active_session(&token_hash)
            .await
            .unwrap()
            .is_some()
    );
    store
        .record_login_attempt(AdminLoginAttempt {
            id: Uuid::new_v4(),
            principal_id: Some(principal_id),
            account_key_hash: SecretHash::new([51; 32]),
            source_ip_hash: SecretHash::new([52; 32]),
            outcome: AdminLoginOutcome::Succeeded,
        })
        .await
        .unwrap();
    store
        .record_security_event(AdminSecurityEvent {
            id: Uuid::new_v4(),
            principal_id: Some(principal_id),
            session_id: store
                .find_active_session(&token_hash)
                .await
                .unwrap()
                .map(|session| session.id),
            source_ip_hash: Some(SecretHash::new([52; 32])),
            event_type: AdminSecurityEventType::SessionCreated,
        })
        .await
        .unwrap();

    let pool = PgPool::connect(&database_url).await.unwrap();
    assert!(sqlx::query("INSERT INTO admin_password_credentials (id, principal_id, password_hash) VALUES ($1, $2, $3)")
        .bind(Uuid::new_v4()).bind(principal_id).bind("not-a-hash")
        .execute(&pool).await.is_err());
    assert!(sqlx::query("INSERT INTO admin_sessions (id, principal_id, token_hash, expires_at) VALUES ($1, $2, $3, $4::timestamptz)")
        .bind(Uuid::new_v4()).bind(principal_id).bind(vec![1_u8; 31]).bind("2035-01-01T00:00:00Z")
        .execute(&pool).await.is_err());
    pool.close().await;
}

/// Verifies protected bootstrap is atomic, one-time, and safe under concurrent PostgreSQL attempts.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_admin_bootstrap_is_atomic_and_idempotent() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAdminStore::connect(&database_url)
        .await
        .expect("admin migrations must succeed");
    let pool = PgPool::connect(&database_url).await.unwrap();
    sqlx::query(
        "TRUNCATE admin_request_records, admin_security_events, admin_password_credentials, admin_sessions, admin_login_attempts, admin_principals",
    )
    .execute(&pool)
    .await
    .unwrap();

    let first = AdminBootstrapConfig::from_values(
        Some("bootstrap-admin".to_owned()),
        Some("a-deterministic-test-password".to_owned()),
    )
    .unwrap();
    let second = AdminBootstrapConfig::from_values(
        Some("other-bootstrap-admin".to_owned()),
        Some("a-different-test-password".to_owned()),
    )
    .unwrap();
    let (first, second) = tokio::join!(
        bootstrap_admin(&store, first),
        bootstrap_admin(&store, second)
    );
    assert!(
        matches!(first, Ok(rockserver::admin::AdminBootstrapOutcome::Created))
            ^ matches!(
                second,
                Ok(rockserver::admin::AdminBootstrapOutcome::Created)
            )
    );
    assert!(
        matches!(
            first,
            Ok(rockserver::admin::AdminBootstrapOutcome::AlreadyExists)
        ) ^ matches!(
            second,
            Ok(rockserver::admin::AdminBootstrapOutcome::AlreadyExists)
        )
    );

    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admin_principals")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admin_password_credentials")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM admin_security_events WHERE event_type = 'admin_created'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    let audit = store
        .list_audit(AdminAuditFilter {
            from: None,
            until: None,
            action: Some("/api/v1/admin/auth/refresh".to_owned()),
            outcome: Some("succeeded".to_owned()),
            offset: 0,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].action, "/api/v1/admin/auth/refresh");
    pool.close().await;
}

/// Exercises administrator login lookup, durable throttle count, and immediate session revocation.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_admin_auth_queries_are_durable_and_revocable() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAdminStore::connect(&database_url).await.unwrap();
    let pool = PgPool::connect(&database_url).await.unwrap();
    sqlx::query("TRUNCATE admin_request_records, admin_security_events, admin_password_credentials, admin_sessions, admin_login_attempts, admin_principals")
        .execute(&pool).await.unwrap();
    let config = AdminBootstrapConfig::from_values(
        Some("auth-admin".to_owned()),
        Some("a-deterministic-test-password".to_owned()),
    )
    .unwrap();
    assert!(matches!(
        bootstrap_admin(&store, config).await,
        Ok(rockserver::admin::AdminBootstrapOutcome::Created)
    ));
    let username = AdminUsername::parse("auth-admin".to_owned()).unwrap();
    let credential = store
        .login_credential(&username)
        .await
        .unwrap()
        .expect("active bootstrap credential");
    let account_hash = SecretHash::new([61; 32]);
    let source_hash = SecretHash::new([62; 32]);
    store
        .record_login_attempt(AdminLoginAttempt {
            id: Uuid::new_v4(),
            principal_id: Some(credential.credential.principal_id),
            account_key_hash: account_hash.clone(),
            source_ip_hash: source_hash.clone(),
            outcome: AdminLoginOutcome::Failed,
        })
        .await
        .unwrap();
    assert_eq!(
        store
            .recent_failed_login_count(&account_hash, &source_hash)
            .await
            .unwrap(),
        1
    );
    let token_hash = SecretHash::new([63; 32]);
    let session_id = Uuid::new_v4();
    store
        .create_session(NewAdminSession {
            id: session_id,
            principal_id: credential.credential.principal_id,
            token_hash: token_hash.clone(),
            ttl_seconds: 60,
        })
        .await
        .unwrap();
    let replacement_hash = SecretHash::new([64; 32]);
    let replacement_id = Uuid::new_v4();
    assert!(
        store
            .rotate_session(
                session_id,
                NewAdminSession {
                    id: replacement_id,
                    principal_id: credential.credential.principal_id,
                    token_hash: replacement_hash.clone(),
                    ttl_seconds: 60,
                },
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .find_active_session(&token_hash)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .find_active_session(&replacement_hash)
            .await
            .unwrap()
            .unwrap()
            .id,
        replacement_id
    );
    store
        .record_request(AdminRequestRecord {
            id: Uuid::new_v4(),
            request_id: "admin-integration-request".to_owned(),
            principal_id: credential.credential.principal_id,
            session_id: replacement_id,
            endpoint: "/api/v1/admin/auth/refresh",
            outcome: AdminRequestOutcome::Succeeded,
            duration_ms: 1,
        })
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admin_request_records")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    pool.close().await;
}

/// Covers the approved passkey-only account persistence invariants against PostgreSQL.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_b2_browser_pairing_webauthn_and_rate_limits() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAccountStore::connect(&database_url)
        .await
        .expect("account migrations must succeed");
    let user_id = Uuid::new_v4();
    store.create_user(user_id).await.unwrap();
    let other_user_id = Uuid::new_v4();
    store.create_user(other_user_id).await.unwrap();

    assert!(
        store
            .create_passkey_credential(
                Uuid::new_v4(),
                user_id,
                b"credential-id",
                b"public-key",
                0,
                &[],
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .advance_passkey_sign_count(b"credential-id", 1)
            .await
            .unwrap()
    );
    assert!(
        store
            .find_passkey_credential_for_user(user_id, b"credential-id")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .find_passkey_credential_for_user(other_user_id, b"credential-id")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .find_passkey_credential(other_user_id.as_bytes())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !store
            .advance_passkey_sign_count(b"credential-id", 0)
            .await
            .unwrap()
    );

    let browser_session_id = Uuid::new_v4();
    assert!(
        store
            .create_browser_session(NewBrowserSession {
                session_id: browser_session_id,
                user_id,
                session_token_hash: &SecretHash::new([11; 32]),
                csrf_hash: &SecretHash::new([1; 32]),
                passkey_reauthenticated_at_rfc3339: "2035-01-01T00:00:00Z",
                expires_at_rfc3339: "2035-02-01T00:00:00Z",
            })
            .await
            .unwrap()
    );

    let challenge_id = Uuid::new_v4();
    let challenge_hash = SecretHash::new([2; 32]);
    assert!(
        store
            .create_webauthn_challenge(NewWebAuthnChallenge {
                challenge_id,
                challenge_hash: &challenge_hash,
                state_blob: b"{}",
                ceremony: WebAuthnCeremony::Authentication,
                rp_id: "alex.vault57.ru",
                origin: "https://alex.vault57.ru",
                expires_at_rfc3339: "2035-01-01T00:00:00Z",
                user_id: Some(user_id),
                browser_session_id: Some(browser_session_id),
                pairing_request_id: None,
            })
            .await
            .unwrap()
    );
    assert!(
        store
            .consume_webauthn_challenge(
                challenge_id,
                &challenge_hash,
                WebAuthnCeremony::Authentication,
                "https://alex.vault57.ru",
                "alex.vault57.ru",
            )
            .await
            .unwrap()
    );
    assert!(
        !store
            .consume_webauthn_challenge(
                challenge_id,
                &challenge_hash,
                WebAuthnCeremony::Authentication,
                "https://alex.vault57.ru",
                "alex.vault57.ru",
            )
            .await
            .unwrap()
    );

    let request_id = Uuid::new_v4();
    assert!(
        store
            .create_pairing_request(NewPairingRequest {
                request_id,
                desktop_token_hash: &SecretHash::new([3; 32]),
                approval_secret_hash: &SecretHash::new([4; 32]),
                short_code_hash: &SecretHash::new([5; 32]),
                verification_phrase: "BLUE-MOON",
                device_display_name: "Test desktop",
                device_type: "rockcast_windows",
                app_version: Some("test"),
                expires_at_rfc3339: "2035-01-01T00:00:00Z",
            })
            .await
            .unwrap()
    );
    assert!(
        store
            .lookup_pairing_request(&SecretHash::new([5; 32]))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        !store
            .approve_pairing_request(
                request_id,
                user_id,
                browser_session_id,
                &SecretHash::new([99; 32]),
                "BLUE-MOON",
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .approve_pairing_request(
                request_id,
                user_id,
                browser_session_id,
                &SecretHash::new([4; 32]),
                "BLUE-MOON",
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .complete_pairing(
                request_id,
                &SecretHash::new([99; 32]),
                NewPairingSession {
                    session_id: Uuid::new_v4(),
                    device_id: Uuid::new_v4(),
                    access_hash: &SecretHash::new([98; 32]),
                    access_expires_at_rfc3339: "2035-01-01T00:10:00Z",
                    device_secret_hash: &SecretHash::new([97; 32]),
                },
            )
            .await
            .unwrap()
            == PairingCompletionOutcome::InvalidProof,
        "a wrong desktop proof must not consume the approved request"
    );
    let expired_request_id = Uuid::new_v4();
    assert!(
        store
            .create_pairing_request(NewPairingRequest {
                request_id: expired_request_id,
                desktop_token_hash: &SecretHash::new([12; 32]),
                approval_secret_hash: &SecretHash::new([13; 32]),
                short_code_hash: &SecretHash::new([14; 32]),
                verification_phrase: "SILVER-STAR",
                device_display_name: "Expired desktop",
                device_type: "rockcast_windows",
                app_version: None,
                expires_at_rfc3339: "2035-01-01T00:00:00Z",
            })
            .await
            .unwrap()
    );
    assert!(
        store
            .approve_pairing_request(
                expired_request_id,
                user_id,
                browser_session_id,
                &SecretHash::new([13; 32]),
                "SILVER-STAR",
            )
            .await
            .unwrap()
    );
    let expiry_pool = repository_pool(&database_url).await;
    sqlx::query(
        "UPDATE pairing_requests SET created_at = now() - interval '2 minutes', \
         expires_at = now() - interval '1 minute' WHERE id = $1",
    )
    .bind(expired_request_id)
    .execute(&expiry_pool)
    .await
    .unwrap();
    expiry_pool.close().await;
    assert!(
        store
            .complete_pairing(
                expired_request_id,
                &SecretHash::new([12; 32]),
                NewPairingSession {
                    session_id: Uuid::new_v4(),
                    device_id: Uuid::new_v4(),
                    access_hash: &SecretHash::new([15; 32]),
                    access_expires_at_rfc3339: "2035-01-01T00:10:00Z",
                    device_secret_hash: &SecretHash::new([16; 32]),
                },
            )
            .await
            .unwrap()
            == PairingCompletionOutcome::NoLongerAvailable,
        "an expired approved request must not issue a session"
    );
    let device_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let completion = store
        .complete_pairing(
            request_id,
            &SecretHash::new([3; 32]),
            NewPairingSession {
                session_id,
                device_id,
                access_hash: &SecretHash::new([6; 32]),
                access_expires_at_rfc3339: "2035-01-01T00:10:00Z",
                device_secret_hash: &SecretHash::new([7; 32]),
            },
        )
        .await
        .unwrap();
    let PairingCompletionOutcome::Completed(completion) = completion else {
        panic!("approved request must derive its owner");
    };
    assert_eq!(completion.user_id, user_id);
    assert_eq!(completion.device_id, device_id);
    assert!(
        store
            .complete_pairing(
                request_id,
                &SecretHash::new([3; 32]),
                NewPairingSession {
                    session_id: Uuid::new_v4(),
                    device_id: Uuid::new_v4(),
                    access_hash: &SecretHash::new([8; 32]),
                    access_expires_at_rfc3339: "2035-01-01T00:10:00Z",
                    device_secret_hash: &SecretHash::new([9; 32]),
                },
            )
            .await
            .unwrap()
            == PairingCompletionOutcome::NoLongerAvailable
    );

    let rate_key = SecretHash::new([10; 32]);
    assert!(
        store
            .consume_rate_limit(&rate_key, "2035-01-01T00:00:00Z", "2035-01-01T00:15:00Z", 1,)
            .await
            .unwrap()
    );
    assert!(
        !store
            .consume_rate_limit(&rate_key, "2035-01-01T00:00:00Z", "2035-01-01T00:15:00Z", 1,)
            .await
            .unwrap()
    );
    store.delete_account(user_id).await.unwrap();
    let inspection = repository_pool(&database_url).await;
    let browser_revoked = sqlx::query_scalar::<_, bool>(
        "SELECT revoked_at IS NOT NULL FROM browser_sessions WHERE id = $1",
    )
    .bind(browser_session_id)
    .fetch_one(&inspection)
    .await
    .unwrap();
    assert!(
        browser_revoked,
        "account deletion must revoke browser sessions"
    );
    inspection.close().await;
    store.close().await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_account_session_rotation_deletion_and_ownership() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAccountStore::connect(&database_url)
        .await
        .expect("account migrations must succeed");
    let owner = Uuid::new_v4();
    let foreign_user = Uuid::new_v4();
    store.create_user(owner).await.expect("first user inserts");
    assert!(
        store.create_user(owner).await.is_err(),
        "duplicate account is rejected"
    );
    store
        .create_user(foreign_user)
        .await
        .expect("second user inserts");
    let device = OwnedDevice {
        id: Uuid::new_v4(),
        user_id: owner,
        device_display_name: "Desktop".into(),
        device_type: "windows".into(),
        created_at: "unused".into(),
        last_seen_at: None,
    };
    assert!(
        store
            .create_device(&device)
            .await
            .expect("owner device inserts")
    );
    assert_eq!(
        store
            .find_owned_device(foreign_user, device.id)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        store.find_owned_device(owner, device.id).await.unwrap(),
        Some(device.clone())
    );

    let session = Uuid::new_v4();
    assert!(
        store
            .create_session(NewSession {
                session_id: session,
                user_id: owner,
                device_id: device.id,
                access_hash: &SecretHash::new([1; 32]),
                access_expires_at_rfc3339: "2035-01-01T00:00:00Z",
            })
            .await
            .unwrap()
    );
    assert!(store.delete_account(owner).await.unwrap());
    assert_eq!(
        store.find_owned_device(owner, device.id).await.unwrap(),
        None
    );
    let inspection = repository_pool(&database_url).await;
    let state = sqlx::query_as::<_, (String, bool)>("SELECT u.status, EXISTS(SELECT 1 FROM sessions s WHERE s.user_id = u.id AND s.revoked_at IS NULL) FROM users u WHERE u.id = $1")
        .bind(owner).fetch_one(&inspection).await.unwrap();
    assert_eq!(state, ("deleted".to_owned(), false));
    inspection.close().await;
}

/// Verifies the browser account centre keeps management actions owner-scoped and revocation blocks access.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_browser_account_centre_owns_rename_and_revoke() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAccountStore::connect(&database_url).await.unwrap();
    let owner = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    store.create_user(owner).await.unwrap();
    store.create_user(foreign).await.unwrap();
    let device = OwnedDevice {
        id: Uuid::new_v4(),
        user_id: owner,
        device_display_name: "Living room PC".into(),
        device_type: "rockcast_windows".into(),
        created_at: "unused".into(),
        last_seen_at: None,
    };
    assert!(store.create_device(&device).await.unwrap());
    let cookie = SecretHash::new([41; 32]);
    let csrf = SecretHash::new([42; 32]);
    assert!(
        store
            .create_browser_session(NewBrowserSession {
                session_id: Uuid::new_v4(),
                user_id: owner,
                session_token_hash: &cookie,
                csrf_hash: &csrf,
                passkey_reauthenticated_at_rfc3339: "2035-01-01T00:00:00Z",
                expires_at_rfc3339: "2035-02-01T00:00:00Z"
            })
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .browser_session_user_with_csrf(&cookie, &csrf)
            .await
            .unwrap(),
        Some(owner)
    );
    let listed = store.list_browser_devices(owner).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].device_display_name, "Living room PC");
    assert_eq!(listed[0].session_status, "inactive");
    assert!(
        !store
            .rename_owned_device(foreign, device.id, "Foreign name")
            .await
            .unwrap()
    );
    assert!(
        store
            .rename_owned_device(owner, device.id, "Studio PC")
            .await
            .unwrap()
    );
    assert_eq!(
        store.list_browser_devices(owner).await.unwrap()[0].device_display_name,
        "Studio PC"
    );
    assert!(
        store
            .create_session(NewSession {
                session_id: Uuid::new_v4(),
                user_id: owner,
                device_id: device.id,
                access_hash: &SecretHash::new([44; 32]),
                access_expires_at_rfc3339: "2035-01-01T00:15:00Z",
            })
            .await
            .unwrap()
    );
    assert!(!store.revoke_owned_device(foreign, device.id).await.unwrap());
    assert!(store.revoke_owned_device(owner, device.id).await.unwrap());
    store.close().await;
}

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
        .bind([45; 32].as_slice())
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
    assert_eq!(result.revoked.devices, 1);
    assert_eq!(result.revoked.sessions, 1);
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

/// Exercises migration 0024, Yandex Home OAuth state lifecycle, and encrypted token persistence.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_yandex_home_connection_lifecycle_and_active_owner() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAccountStore::connect(&database_url)
        .await
        .expect("account migrations must succeed");
    let user_id = Uuid::new_v4();
    store
        .create_user_with_display_name(user_id, "yandex test account")
        .await
        .unwrap();

    assert!(!store.has_yandex_home_connection(user_id).await.unwrap());
    assert!(
        store
            .yandex_home_connection(user_id)
            .await
            .unwrap()
            .is_none()
    );

    let state_hash = SecretHash::new([42u8; 32]);
    assert!(
        store
            .create_yandex_home_oauth_state(user_id, &state_hash)
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .consume_yandex_home_oauth_state(&state_hash)
            .await
            .unwrap(),
        Some(user_id)
    );
    assert_eq!(
        store
            .consume_yandex_home_oauth_state(&state_hash)
            .await
            .unwrap(),
        None
    );

    let token = EncryptedYandexHomeToken {
        ciphertext: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17],
        nonce: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
    };
    assert!(
        store
            .save_yandex_home_connection(user_id, &token)
            .await
            .unwrap()
    );
    assert!(store.has_yandex_home_connection(user_id).await.unwrap());
    assert_eq!(
        store.yandex_home_connection(user_id).await.unwrap(),
        Some(token)
    );

    assert!(store.revoke_yandex_home_connection(user_id).await.unwrap());
    assert!(!store.has_yandex_home_connection(user_id).await.unwrap());
    assert!(
        store
            .yandex_home_connection(user_id)
            .await
            .unwrap()
            .is_none()
    );
    store.close().await;
}

/// Exercises migrations, seed data, search semantics, and live database readiness.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_migrations_seed_search_and_readiness() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let repository = PostgresStationRepository::connect(&database_url)
        .await
        .expect("migrations and seed must succeed");
    let service = SearchService::new(Arc::new(repository.clone()));
    let extension_pool = repository_pool(&database_url).await;
    let extension_version = sqlx::query_scalar::<_, String>(
        "SELECT extversion FROM pg_extension WHERE extname = 'vector'",
    )
    .fetch_one(&extension_pool)
    .await
    .expect("pgvector migration must enable the extension");
    assert!(!extension_version.is_empty());
    extension_pool.close().await;

    let rock_query = normalize_query("rock".to_owned(), "en-US".to_owned());
    let rock_results = service
        .search(
            &rock_query,
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("seeded search must succeed");
    assert!(
        rock_results
            .iter()
            .any(|ranked| ranked.station.id == "rock-antenne")
    );

    let limited_results = service
        .search(
            &rock_query,
            &SearchConstraints {
                limit: 1,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("limited search must succeed");
    assert_eq!(
        station_ids(&limited_results),
        station_ids(&rock_results)[..1]
    );

    let metal_query = normalize_query("metal".to_owned(), "en-US".to_owned());
    let exclusion_results = service
        .search(
            &metal_query,
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::from(["somafm-metal-detector".to_owned()]),
            },
        )
        .await
        .expect("excluded search must succeed");
    assert!(!station_ids(&exclusion_results).contains(&"somafm-metal-detector"));

    let embedding_store = PostgresEmbeddingStore::connect(&database_url)
        .await
        .expect("embedding store migrations must succeed");
    embedding_store
        .upsert_embedding(
            "rock-antenne",
            &Embedding::new("integration-model", "1", 3, vec![0.0, 1.0, 0.0]).unwrap(),
        )
        .await
        .expect("first embedding insert must succeed");
    embedding_store
        .upsert_embedding(
            "rock-antenne",
            &Embedding::new("integration-model", "1", 3, vec![1.0, 0.0, 0.0]).unwrap(),
        )
        .await
        .expect("repeat embedding update must succeed");
    for (station_id, values) in [
        ("radio-record-rock", vec![0.0, 1.0, 0.0]),
        ("gotradio-punk-rock", vec![0.0, 0.0, 1.0]),
        ("rock-antenne-alternative", vec![0.0, 0.0, 1.0]),
    ] {
        embedding_store
            .upsert_embedding(
                station_id,
                &Embedding::new("integration-model", "1", 3, values).unwrap(),
            )
            .await
            .expect("station embedding insert must succeed");
    }

    let semantic_service = SearchService::with_providers(
        Arc::new(repository.clone()),
        Arc::new(DeterministicQueryParser),
        Some(Arc::new(FixedEmbeddingProvider)),
    );
    let semantic_query = SearchQuery {
        action: SearchAction::Play,
        original: "rock".to_owned(),
        locale: "en-US".to_owned(),
        terms: vec!["rock".to_owned()],
        tags: Vec::new(),
        language: None,
        country_code: None,
        core_term_count: 1,
        raw_query: "rock".to_owned(),
        prefer_station_name: false,
        station_name_hint_queries: Vec::new(),
    };
    let semantic_results = semantic_service
        .search(
            &semantic_query,
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("semantic search must succeed");
    assert!(!semantic_results.is_empty());

    let british_semantic_results = semantic_service
        .search(
            &SearchQuery {
                country_code: Some("GB".to_owned()),
                ..semantic_query.clone()
            },
            &SearchConstraints {
                limit: 1,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("hard-filtered semantic search must succeed");
    assert!(
        british_semantic_results
            .iter()
            .all(|station| station.station.country_code.as_deref() == Some("GB"))
    );

    let tie_results = semantic_service
        .search(
            &semantic_query,
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::from([
                    "rock-antenne".to_owned(),
                    "radio-record-rock".to_owned(),
                ]),
            },
        )
        .await
        .expect("semantic tie search must succeed");
    assert!(!tie_results.is_empty());

    let fallback_service = SearchService::with_providers(
        Arc::new(repository.clone()),
        Arc::new(DeterministicQueryParser),
        Some(Arc::new(FailingEmbeddingProvider)),
    );
    let fallback_results = fallback_service
        .search(
            &rock_query,
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("provider failure must retain metadata search");
    assert!(!station_ids(&fallback_results).is_empty());

    let embedding_pool = repository_pool(&database_url).await;
    let persisted_count = sqlx::query_scalar::<_, i64>(
        r#"
SELECT COUNT(*)
FROM station_embeddings
WHERE station_id = 'rock-antenne'
  AND model = 'integration-model'
  AND version = '1'
"#,
    )
    .fetch_one(&embedding_pool)
    .await
    .expect("embedding count must be inspectable");
    let persisted = sqlx::query_as::<_, (i32, f64, bool)>(
        r#"
SELECT dimension, embedding <=> '[1,0,0]'::vector, updated_at >= created_at
FROM station_embeddings
WHERE station_id = 'rock-antenne'
  AND model = 'integration-model'
  AND version = '1'
"#,
    )
    .fetch_one(&embedding_pool)
    .await
    .expect("embedding persistence must be inspectable");
    assert_eq!(persisted_count, 1);
    assert_eq!(persisted.0, 3);
    assert!(persisted.1.abs() < 0.000_001);
    assert!(persisted.2);
    embedding_pool.close().await;
    embedding_store.close().await;

    let import_store = PostgresImportStore::connect(&database_url)
        .await
        .expect("import store migrations must succeed");
    let source_station_id = "01234567-89ab-cdef-0123-456789abcdef";
    let first_import = CatalogImporter::new(
        OnePageProvider::station(imported_station(
            source_station_id,
            "Integration Rock Radio",
            "https://streams.example.com/imported-first.mp3",
            &["rock"],
        )),
        import_store.clone(),
        ImportLimits {
            page_size: 10,
            max_pages: 2,
        },
    )
    .run()
    .await
    .expect("first import must succeed");
    let repeat_import = CatalogImporter::new(
        OnePageProvider::station(imported_station(
            source_station_id,
            "Integration Rock Radio Updated",
            "https://streams.example.com/imported-updated.mp3",
            &["rock", "upbeat"],
        )),
        import_store.clone(),
        ImportLimits {
            page_size: 10,
            max_pages: 2,
        },
    )
    .run()
    .await
    .expect("repeat import must update in place");
    let failed_import = CatalogImporter::new(
        OnePageProvider::failure(),
        import_store.clone(),
        ImportLimits {
            page_size: 10,
            max_pages: 2,
        },
    )
    .run()
    .await
    .expect_err("provider failure must fail the run");
    assert_eq!(failed_import.safe_summary(), "mock provider unavailable");

    let inspection_pool = PgPool::connect(&database_url)
        .await
        .expect("inspection connection must succeed");
    let shared_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM stations WHERE source = 'rockcatalog'")
            .fetch_one(&inspection_pool)
            .await
            .unwrap();
    assert_eq!(
        shared_count, 41,
        "shared release must remain active beside imports"
    );
    let provider_station_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM stations WHERE source = $1 AND source_station_id = $2",
    )
    .bind(SOURCE)
    .bind(source_station_id)
    .fetch_one(&inspection_pool)
    .await
    .unwrap();
    assert_eq!(
        provider_station_count, 1,
        "repeat import must not duplicate"
    );
    let provider_stream = sqlx::query_as::<_, (String, String)>(
        r#"
SELECT s.name, ss.stream_url
FROM stations AS s
JOIN station_streams AS ss ON ss.station_id = s.id
WHERE s.source = $1 AND s.source_station_id = $2
"#,
    )
    .bind(SOURCE)
    .bind(source_station_id)
    .fetch_one(&inspection_pool)
    .await
    .unwrap();
    assert_eq!(provider_stream.0, "Integration Rock Radio Updated");
    assert_eq!(
        provider_stream.1,
        "https://streams.example.com/imported-updated.mp3"
    );

    assert_run(
        &inspection_pool,
        &first_import.run_id,
        "completed",
        (1, 1, 0, 0),
        false,
    )
    .await;
    assert_run(
        &inspection_pool,
        &repeat_import.run_id,
        "completed",
        (1, 1, 0, 0),
        false,
    )
    .await;
    let failed_run_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM import_runs WHERE source = $1 AND status = 'failed' ORDER BY started_at DESC LIMIT 1",
    )
    .bind(SOURCE)
    .fetch_one(&inspection_pool)
    .await
    .unwrap()
    .to_string();
    assert_run(
        &inspection_pool,
        &failed_run_id,
        "failed",
        (0, 0, 0, 0),
        true,
    )
    .await;

    let imported_query = normalize_query("upbeat".to_owned(), "en-US".to_owned());
    let imported_results = service
        .search(
            &imported_query,
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("search over imported metadata must succeed");
    assert!(
        imported_results
            .iter()
            .any(|ranked| ranked.station.id == format!("rb-{source_station_id}")),
        "imported station must be searchable"
    );
    inspection_pool.close().await;
    import_store.close().await;

    let app = router_with_repository(Arc::new(repository.clone()));
    let ready = app
        .clone()
        .oneshot(Request::get("/health/ready").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(ready.status(), axum::http::StatusCode::OK);

    let provider_independent_ready = router_with_search_service(fallback_service)
        .oneshot(Request::get("/health/ready").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        provider_independent_ready.status(),
        axum::http::StatusCode::OK
    );

    repository.close().await;

    let unavailable = app
        .oneshot(Request::get("/health/ready").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        unavailable.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    let body = unavailable.into_body().collect().await.unwrap().to_bytes();
    let payload: HealthResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.status, HealthStatus::NotReady);

    let response = router_with_repository(Arc::new(
        PostgresStationRepository::connect(&database_url)
            .await
            .expect("repository should reconnect"),
    ))
    .oneshot(
        Request::post("/api/v1/search")
            .header("content-type", "application/json")
            .header(
                "authorization",
                format!("Bearer {}", rockserver::http::TEST_API_BEARER_TOKEN),
            )
            .body(Body::from(r#"{"query":"rock"}"#))
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        payload["stations"][0]["id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
}

/// Exercises canonical retirement metadata through transactional activation, search, rollback, and
/// provider coexistence against a disposable database.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn shared_catalog_tombstones_are_active_idempotent_and_rollback_safe() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let repository = PostgresStationRepository::connect(&database_url)
        .await
        .expect("baseline activation must succeed");
    let store = PostgresImportStore::connect(&database_url)
        .await
        .expect("catalog store must connect");
    let before = lifecycle_catalog("test-before", false);
    let after = lifecycle_catalog("test-after", true);

    store
        .activate_shared_catalog(&before)
        .await
        .expect("initial lifecycle release must activate");
    let radio_browser_import = CatalogImporter::new(
        OnePageProvider::station(imported_station(
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "Provider ownership sentinel",
            "https://streams.example.com/ownership-sentinel.mp3",
            &["rock"],
        )),
        store.clone(),
        ImportLimits {
            page_size: 10,
            max_pages: 1,
        },
    );
    radio_browser_import
        .run()
        .await
        .expect("Radio Browser sentinel must import");

    store
        .activate_shared_catalog(&after)
        .await
        .expect("retirement release must activate");
    store
        .activate_shared_catalog(&after)
        .await
        .expect("re-import of the same release must be idempotent");

    assert_eq!(
        store
            .lookup_shared_catalog_replacement("merge-old")
            .await
            .unwrap(),
        OwnedCatalogReplacement::Redirect("merge-target".to_owned())
    );
    assert_eq!(
        store
            .lookup_shared_catalog_replacement("split-old")
            .await
            .unwrap(),
        OwnedCatalogReplacement::Ambiguous(vec!["split-one".to_owned(), "split-two".to_owned()])
    );
    assert_eq!(
        store
            .lookup_shared_catalog_replacement("removed-old")
            .await
            .unwrap(),
        OwnedCatalogReplacement::Removed
    );

    let service = SearchService::new(Arc::new(repository.clone()));
    let retired_results = service
        .search(
            &normalize_query("legacy retired marker".to_owned(), "en-US".to_owned()),
            &SearchConstraints {
                limit: 10,
                excluded_station_ids: BTreeSet::new(),
            },
        )
        .await
        .expect("retired rows must be filtered rather than breaking search");
    assert!(retired_results.iter().all(|station| !matches!(
        station.station.id.as_str(),
        "removed-old" | "merge-old" | "split-old"
    )));

    let pool = repository_pool(&database_url).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM catalog_tombstones WHERE source = 'rockcatalog'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        3
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM stations WHERE source = 'radio_browser' AND source_station_id = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1,
        "RockCatalog activation must not change Radio Browser ownership"
    );

    store
        .activate_shared_catalog(&before)
        .await
        .expect("previous release rollback must reactivate its rows");
    assert_eq!(
        store
            .lookup_shared_catalog_replacement("merge-old")
            .await
            .unwrap(),
        OwnedCatalogReplacement::Unknown
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM stations WHERE source = 'rockcatalog' AND source_station_id = 'merge-old' AND retired_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    pool.close().await;
    store.close().await;
    repository.close().await;
}

/// Builds two immutable fixtures that model a prior release and a replacement release.
fn lifecycle_catalog(version: &str, with_tombstones: bool) -> PinnedSharedCatalog {
    let retired = if with_tombstones {
        ""
    } else {
        r#",
    {"id":"removed-old","name":"Legacy retired marker","aliases":[],"legacyIds":[],"tags":["rock"],"countryCode":null,"language":null,"homepageUrl":null,"faviconUrl":null,"streams":[{"id":"main","url":"https://example.com/removed-old","codec":"mp3","bitrateKbps":128,"primary":true}]},
    {"id":"merge-old","name":"Legacy merge marker","aliases":[],"legacyIds":[],"tags":["rock"],"countryCode":null,"language":null,"homepageUrl":null,"faviconUrl":null,"streams":[{"id":"main","url":"https://example.com/merge-old","codec":"mp3","bitrateKbps":128,"primary":true}]},
    {"id":"split-old","name":"Legacy split marker","aliases":[],"legacyIds":[],"tags":["rock"],"countryCode":null,"language":null,"homepageUrl":null,"faviconUrl":null,"streams":[{"id":"main","url":"https://example.com/split-old","codec":"mp3","bitrateKbps":128,"primary":true}]}"#
    };
    let tombstones = if with_tombstones {
        r#"[{"id":"removed-old","reason":"removed","replacementIds":[]},{"id":"merge-old","reason":"merged","replacementIds":["merge-target"]},{"id":"split-old","reason":"split","replacementIds":["split-one","split-two"]}]"#
    } else {
        "[]"
    };
    let document = format!(
        r#"{{"schemaVersion":1,"catalogVersion":"{version}","stations":[
    {{"id":"merge-target","name":"Merge target","aliases":[],"legacyIds":[],"tags":["rock"],"countryCode":null,"language":null,"homepageUrl":null,"faviconUrl":null,"streams":[{{"id":"main","url":"https://example.com/merge-target","codec":"mp3","bitrateKbps":128,"primary":true}}]}},
    {{"id":"split-one","name":"Split one","aliases":[],"legacyIds":[],"tags":["rock"],"countryCode":null,"language":null,"homepageUrl":null,"faviconUrl":null,"streams":[{{"id":"main","url":"https://example.com/split-one","codec":"mp3","bitrateKbps":128,"primary":true}}]}},
    {{"id":"split-two","name":"Split two","aliases":[],"legacyIds":[],"tags":["rock"],"countryCode":null,"language":null,"homepageUrl":null,"faviconUrl":null,"streams":[{{"id":"main","url":"https://example.com/split-two","codec":"mp3","bitrateKbps":128,"primary":true}}]}}{retired}],"tombstones":{tombstones}}}"#,
    );
    let digest = format!("{:x}", sha2::Sha256::digest(document.as_bytes()));
    PinnedSharedCatalog::from_bytes(document.as_bytes(), version, &digest).unwrap()
}

fn station_ids(results: &[rockserver::search::RankedStation]) -> Vec<&str> {
    results
        .iter()
        .map(|ranked| ranked.station.id.as_str())
        .collect()
}

async fn repository_pool(database_url: &str) -> PgPool {
    PgPool::connect(database_url)
        .await
        .expect("inspection connection must succeed")
}

struct FixedEmbeddingProvider;

#[async_trait]
impl EmbeddingProvider for FixedEmbeddingProvider {
    async fn embed(&self, _text: &str) -> Result<Embedding, EmbeddingProviderError> {
        Ok(Embedding::new("integration-model", "1", 3, vec![1.0, 0.0, 0.0]).unwrap())
    }
}

struct FailingEmbeddingProvider;

#[async_trait]
impl EmbeddingProvider for FailingEmbeddingProvider {
    async fn embed(&self, _text: &str) -> Result<Embedding, EmbeddingProviderError> {
        Err(EmbeddingProviderError::safe(
            "scripted integration provider failure",
        ))
    }
}

struct OnePageProvider {
    station: Option<ImportedStation>,
    fail: bool,
}

impl OnePageProvider {
    fn station(station: ImportedStation) -> Self {
        Self {
            station: Some(station),
            fail: false,
        }
    }

    fn failure() -> Self {
        Self {
            station: None,
            fail: true,
        }
    }
}

#[async_trait]
impl CatalogImportProvider for OnePageProvider {
    fn source(&self) -> &'static str {
        SOURCE
    }

    async fn fetch_page(
        &self,
        _offset: usize,
        _limit: usize,
    ) -> Result<ImportPage, CatalogImportError> {
        if self.fail {
            return Err(CatalogImportError::safe("mock provider unavailable"));
        }
        Ok(ImportPage {
            fetched: usize::from(self.station.is_some()),
            stations: self.station.clone().into_iter().collect(),
            skipped: 0,
        })
    }
}

fn imported_station(
    source_station_id: &str,
    name: &str,
    stream_url: &str,
    tags: &[&str],
) -> ImportedStation {
    ImportedStation {
        source: SOURCE,
        source_station_id: source_station_id.to_owned(),
        id: format!("rb-{source_station_id}"),
        name: name.to_owned(),
        homepage_url: Some("https://example.com/imported-radio".to_owned()),
        favicon_source_url: Some("https://icons.example.com/imported-radio.png".to_owned()),
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        language: Some("en".to_owned()),
        country_code: Some("US".to_owned()),
        streams: vec![ImportedStream {
            source_stream_id: source_station_id.to_owned(),
            stream_url: stream_url.to_owned(),
            codec: Some("MP3".to_owned()),
            bitrate_kbps: Some(192),
            is_primary: true,
        }],
    }
}

async fn assert_run(
    pool: &PgPool,
    run_id: &str,
    expected_status: &str,
    expected_counts: (i64, i64, i64, i64),
    expects_error: bool,
) {
    let run_id = Uuid::parse_str(run_id).unwrap();
    let row = sqlx::query_as::<_, (String, i64, i64, i64, i64, Option<String>, bool)>(
        r#"
SELECT status, fetched_count, imported_count, skipped_count, failed_count, error_summary,
       completed_at IS NOT NULL
FROM import_runs
WHERE id = $1
"#,
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(row.0, expected_status);
    assert_eq!((row.1, row.2, row.3, row.4), expected_counts);
    assert_eq!(row.5.is_some(), expects_error);
    assert!(row.6, "terminal run must have completed_at");
}

/// Exercises migration 0025: last-writer-wins merges, per-account cursor deltas, tombstone
/// propagation, retention expiry, and deleted-account purge against real PostgreSQL.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_personal_data_sync_is_last_writer_wins_and_cursor_scoped() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let accounts = PostgresAccountStore::connect(&database_url).await.unwrap();
    let store = PostgresPersonalDataStore::connect(&database_url)
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    accounts.create_user(owner).await.unwrap();
    accounts.create_user(foreign).await.unwrap();

    let now = time::OffsetDateTime::now_utc();
    let stamp = |offset: time::Duration| {
        PersonalTimestamp::parse(
            (now + offset)
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
        )
        .unwrap()
    };
    let favourite_record = Uuid::new_v4();
    let history_record = Uuid::new_v4();

    // First device pushes both collections.
    let pushed = store
        .synchronize(
            owner,
            SyncRequest {
                since_revision: 0,
                favourites: CollectionChanges {
                    upserts: vec![FavouriteUpsert {
                        record_id: favourite_record,
                        station_id: "station-rock-001".into(),
                        added_at: stamp(time::Duration::ZERO),
                        updated_at: stamp(time::Duration::ZERO),
                    }],
                    deletes: vec![],
                },
                history: CollectionChanges {
                    upserts: vec![HistoryUpsert {
                        record_id: history_record,
                        station_id: "station-jazz-002".into(),
                        started_at: stamp(time::Duration::minutes(-30)),
                        last_played_at: stamp(time::Duration::minutes(-5)),
                        ended_at: None,
                        play_duration_ms: Some(1_500_000),
                        metadata: Some(serde_json::json!({"lastKnownName": "Jazz FM"})),
                        updated_at: stamp(time::Duration::ZERO),
                    }],
                    deletes: vec![],
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(pushed.favourites.len(), 1);
    assert_eq!(pushed.history.len(), 1);
    assert!(pushed.server_revision > 0);
    let cursor = pushed.server_revision;

    // The same account from another device pulls a full snapshot.
    let snapshot = store
        .synchronize(owner, SyncRequest::default())
        .await
        .unwrap();
    assert_eq!(snapshot.favourites.len(), 1);
    assert_eq!(snapshot.history.len(), 1);
    assert_eq!(snapshot.server_revision, cursor);
    // A delta from the cursor returns nothing new.
    let quiet = store
        .synchronize(
            owner,
            SyncRequest {
                since_revision: cursor,
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert!(quiet.favourites.is_empty() && quiet.history.is_empty());

    // A stale upsert loses and is echoed back without advancing the cursor.
    let stale = store
        .synchronize(
            owner,
            SyncRequest {
                since_revision: cursor,
                favourites: CollectionChanges {
                    upserts: vec![FavouriteUpsert {
                        record_id: favourite_record,
                        station_id: "station-old".into(),
                        added_at: stamp(time::Duration::hours(-1)),
                        updated_at: stamp(time::Duration::hours(-1)),
                    }],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        stale.favourites[0].station_id.as_deref(),
        Some("station-rock-001")
    );
    assert_eq!(stale.server_revision, cursor);

    // Deletions propagate as tombstones and stale upserts cannot resurrect.
    let deleted = store
        .synchronize(
            owner,
            SyncRequest {
                since_revision: cursor,
                favourites: CollectionChanges {
                    upserts: vec![],
                    deletes: vec![RecordDelete {
                        record_id: favourite_record,
                        updated_at: stamp(time::Duration::minutes(1)),
                    }],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert!(deleted.favourites[0].deleted_at.is_some());
    let resurrected = store
        .synchronize(
            owner,
            SyncRequest {
                since_revision: deleted.server_revision,
                favourites: CollectionChanges {
                    upserts: vec![FavouriteUpsert {
                        record_id: favourite_record,
                        station_id: "station-rock-001".into(),
                        added_at: stamp(time::Duration::hours(-2)),
                        updated_at: stamp(time::Duration::hours(-2)),
                    }],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert!(resurrected.favourites[0].deleted_at.is_some());

    // Accounts are isolated.
    let other = store
        .synchronize(foreign, SyncRequest::default())
        .await
        .unwrap();
    assert!(other.favourites.is_empty() && other.history.is_empty());

    // History past the retention window expires server-side on the next push.
    let ancient = store
        .synchronize(
            owner,
            SyncRequest {
                history: CollectionChanges {
                    upserts: vec![HistoryUpsert {
                        record_id: Uuid::new_v4(),
                        station_id: "station-ancient".into(),
                        started_at: stamp(time::Duration::days(-200)),
                        last_played_at: stamp(time::Duration::days(-200)),
                        ended_at: None,
                        play_duration_ms: None,
                        metadata: None,
                        updated_at: stamp(time::Duration::days(-200)),
                    }],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        ancient.history.len(),
        2,
        "since_revision 0 returns the full snapshot: both history rows"
    );
    assert!(
        ancient
            .history
            .iter()
            .any(|row| row.station_id.as_deref() == Some("station-ancient")
                && row.deleted_at.is_some()),
        "push-time retention must tombstone ancient history immediately"
    );

    // The fleet-wide sweep expires rows whose clock aged past the window after the push.
    let raw = sqlx::PgPool::connect(&database_url).await.unwrap();
    sqlx::query(
        "UPDATE history_records SET started_at = now() - make_interval(days => 200) \
         WHERE user_id=$1 AND deleted_at IS NULL",
    )
    .bind(owner)
    .execute(&raw)
    .await
    .unwrap();
    let sweep = store.sweep_retention().await.unwrap();
    assert!(
        sweep.history_expired >= 1,
        "the sweep must expire aged rows"
    );
    let after_sweep = store
        .synchronize(owner, SyncRequest::default())
        .await
        .unwrap();
    assert!(
        after_sweep
            .history
            .iter()
            .all(|row| row.deleted_at.is_some()),
        "every history row must be tombstoned after expiry"
    );

    // Rows of accounts deleted past the grace window are purged for good.
    let doomed = Uuid::new_v4();
    accounts.create_user(doomed).await.unwrap();
    store
        .synchronize(
            doomed,
            SyncRequest {
                favourites: CollectionChanges {
                    upserts: vec![FavouriteUpsert {
                        record_id: Uuid::new_v4(),
                        station_id: "station-doomed".into(),
                        added_at: stamp(time::Duration::ZERO),
                        updated_at: stamp(time::Duration::ZERO),
                    }],
                    deletes: vec![],
                },
                ..SyncRequest::default()
            },
        )
        .await
        .unwrap();
    let raw = sqlx::PgPool::connect(&database_url).await.unwrap();
    sqlx::query("UPDATE users SET status='deleted', deleted_at=now() - make_interval(days => 40) WHERE id=$1")
        .bind(doomed)
        .execute(&raw)
        .await
        .unwrap();
    let sweep = store.sweep_retention().await.unwrap();
    assert!(
        sweep.deleted_account_rows_removed >= 1,
        "deleted-account rows must be purged"
    );
    let purged = store
        .synchronize(doomed, SyncRequest::default())
        .await
        .unwrap();
    assert!(
        purged.favourites.is_empty(),
        "purged rows must stop being returned"
    );
}
