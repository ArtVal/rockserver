use uuid::Uuid;

use crate::auth::SecretHash;

use super::*;

#[test]
fn password_hash_requires_argon2id_and_redacts_debug() {
    assert_eq!(
        AdminPasswordHash::parse("password".to_owned()),
        Err(AdminPasswordHashError::NotArgon2id)
    );
    let hash = AdminPasswordHash::parse("$argon2id$v=19$m=1,t=1,p=1$salt$hash".to_owned()).unwrap();
    assert_eq!(format!("{hash:?}"), "AdminPasswordHash([REDACTED])");
}

#[tokio::test]
async fn fake_store_requires_a_principal_and_never_accepts_raw_secret_material() {
    let store = FakeAdminStore::default();
    let principal_id = Uuid::new_v4();
    let credential = NewAdminPasswordCredential {
        id: Uuid::new_v4(),
        principal_id,
        password_hash: AdminPasswordHash::parse("$argon2id$v=19$m=1,t=1,p=1$salt$hash".to_owned())
            .unwrap(),
    };
    assert_eq!(
        store.create_password_credential(credential.clone()).await,
        Err(AdminStoreError::NotFound)
    );
    store
        .create_principal(AdminPrincipal {
            id: principal_id,
            status: AdminPrincipalStatus::Active,
        })
        .await
        .unwrap();
    store.create_password_credential(credential).await.unwrap();
    let token_hash = SecretHash::new([9; 32]);
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
}

#[tokio::test]
async fn fake_store_rejects_sessions_for_disabled_principals() {
    let store = FakeAdminStore::default();
    let principal_id = Uuid::new_v4();
    store
        .create_principal(AdminPrincipal {
            id: principal_id,
            status: AdminPrincipalStatus::Disabled,
        })
        .await
        .unwrap();
    assert_eq!(
        store
            .create_session(NewAdminSession {
                id: Uuid::new_v4(),
                principal_id,
                token_hash: SecretHash::new([5; 32]),
                ttl_seconds: 60,
            })
            .await,
        Err(AdminStoreError::NotFound)
    );
}

#[tokio::test]
async fn fake_store_rotation_replaces_the_only_active_session_and_records_safe_metadata() {
    let store = FakeAdminStore::default();
    let principal_id = Uuid::new_v4();
    store
        .create_principal(AdminPrincipal {
            id: principal_id,
            status: AdminPrincipalStatus::Active,
        })
        .await
        .unwrap();
    let old_hash = SecretHash::new([7; 32]);
    let old_session_id = Uuid::new_v4();
    store
        .create_session(NewAdminSession {
            id: old_session_id,
            principal_id,
            token_hash: old_hash.clone(),
            ttl_seconds: 60,
        })
        .await
        .unwrap();
    let new_hash = SecretHash::new([8; 32]);
    let replacement_id = Uuid::new_v4();
    assert!(
        store
            .rotate_session(
                old_session_id,
                NewAdminSession {
                    id: replacement_id,
                    principal_id,
                    token_hash: new_hash.clone(),
                    ttl_seconds: 60,
                },
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .find_active_session(&old_hash)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .find_active_session(&new_hash)
            .await
            .unwrap()
            .unwrap()
            .id,
        replacement_id
    );
    store
        .record_request(AdminRequestRecord {
            id: Uuid::new_v4(),
            request_id: "request_123".to_owned(),
            principal_id,
            session_id: replacement_id,
            endpoint: "/api/v1/admin/stations",
            outcome: AdminRequestOutcome::Succeeded,
            duration_ms: 12,
        })
        .await
        .unwrap();
    assert_eq!(store.request_records().len(), 1);
}
