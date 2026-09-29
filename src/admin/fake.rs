//! Deterministic in-memory administrator store for unit tests and local execution.

use std::{collections::BTreeMap, sync::Mutex};

use async_trait::async_trait;
use uuid::Uuid;

use crate::auth::SecretHash;

use super::domain::{
    AdminAuditEntry, AdminAuditFilter, AdminBootstrapOutcome, AdminLoginAttempt,
    AdminLoginCredential, AdminLoginOutcome, AdminPasswordCredential, AdminPrincipal,
    AdminPrincipalStatus, AdminRequestRecord, AdminSecurityEvent, AdminSecurityEventType,
    AdminSession, AdminStore, AdminStoreError, AdminUsername, NewAdminBootstrap,
    NewAdminPasswordCredential, NewAdminSession,
};

/// Deterministic in-memory administrator store for unit tests.
#[derive(Default)]
pub struct FakeAdminStore {
    state: Mutex<FakeAdminState>,
}

#[derive(Default)]
struct FakeAdminState {
    principals: BTreeMap<Uuid, AdminPrincipal>,
    usernames: BTreeMap<String, Uuid>,
    credentials: BTreeMap<Uuid, AdminPasswordCredential>,
    sessions: BTreeMap<SecretHashKey, AdminSession>,
    login_attempts: Vec<AdminLoginAttempt>,
    security_events: Vec<AdminSecurityEvent>,
    request_records: Vec<AdminRequestRecord>,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct SecretHashKey([u8; 32]);

impl From<&SecretHash> for SecretHashKey {
    fn from(value: &SecretHash) -> Self {
        let mut bytes = [0; 32];
        bytes.copy_from_slice(value.as_bytes());
        Self(bytes)
    }
}

impl FakeAdminStore {
    /// Returns a deterministic snapshot of all recorded login attempts.
    pub fn login_attempts(&self) -> Vec<AdminLoginAttempt> {
        self.state
            .lock()
            .expect("fake admin store lock poisoned")
            .login_attempts
            .clone()
    }

    /// Returns a deterministic snapshot of all recorded security events.
    pub fn security_events(&self) -> Vec<AdminSecurityEvent> {
        self.state
            .lock()
            .expect("fake admin store lock poisoned")
            .security_events
            .clone()
    }

    /// Returns a deterministic snapshot of bounded administrator request records.
    pub fn request_records(&self) -> Vec<AdminRequestRecord> {
        self.state
            .lock()
            .expect("fake admin store lock poisoned")
            .request_records
            .clone()
    }
}

#[async_trait]
impl AdminStore for FakeAdminStore {
    async fn bootstrap_admin(
        &self,
        bootstrap: NewAdminBootstrap,
    ) -> Result<AdminBootstrapOutcome, AdminStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?;
        if !state.principals.is_empty() {
            return Ok(AdminBootstrapOutcome::AlreadyExists);
        }
        state.principals.insert(
            bootstrap.principal_id,
            AdminPrincipal {
                id: bootstrap.principal_id,
                status: AdminPrincipalStatus::Active,
            },
        );
        state
            .usernames
            .insert(bootstrap.username.0, bootstrap.principal_id);
        state.credentials.insert(
            bootstrap.principal_id,
            AdminPasswordCredential {
                id: bootstrap.credential_id,
                principal_id: bootstrap.principal_id,
                password_hash: bootstrap.password_hash,
            },
        );
        state.security_events.push(AdminSecurityEvent {
            id: bootstrap.security_event_id,
            principal_id: Some(bootstrap.principal_id),
            session_id: None,
            source_ip_hash: None,
            event_type: AdminSecurityEventType::AdminCreated,
        });
        Ok(AdminBootstrapOutcome::Created)
    }

    async fn create_principal(&self, principal: AdminPrincipal) -> Result<(), AdminStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?;
        if state.principals.insert(principal.id, principal).is_some() {
            return Err(AdminStoreError::Conflict);
        }
        Ok(())
    }

    async fn create_password_credential(
        &self,
        credential: NewAdminPasswordCredential,
    ) -> Result<(), AdminStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?;
        if !state.principals.contains_key(&credential.principal_id) {
            return Err(AdminStoreError::NotFound);
        }
        if state.credentials.contains_key(&credential.principal_id) {
            return Err(AdminStoreError::Conflict);
        }
        state.credentials.insert(
            credential.principal_id,
            AdminPasswordCredential {
                id: credential.id,
                principal_id: credential.principal_id,
                password_hash: credential.password_hash,
            },
        );
        Ok(())
    }

    async fn active_password_credential(
        &self,
        principal_id: Uuid,
    ) -> Result<Option<AdminPasswordCredential>, AdminStoreError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?
            .credentials
            .get(&principal_id)
            .cloned())
    }

    async fn login_credential(
        &self,
        username: &AdminUsername,
    ) -> Result<Option<AdminLoginCredential>, AdminStoreError> {
        let state = self
            .state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?;
        Ok(state
            .usernames
            .get(&username.0)
            .and_then(|id| state.credentials.get(id))
            .cloned()
            .map(|credential| AdminLoginCredential { credential }))
    }

    async fn create_session(&self, session: NewAdminSession) -> Result<(), AdminStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?;
        if !matches!(
            state.principals.get(&session.principal_id),
            Some(AdminPrincipal {
                status: AdminPrincipalStatus::Active,
                ..
            })
        ) {
            return Err(AdminStoreError::NotFound);
        }
        let key = SecretHashKey::from(&session.token_hash);
        if state.sessions.contains_key(&key) {
            return Err(AdminStoreError::Conflict);
        }
        state.sessions.insert(
            key,
            AdminSession {
                id: session.id,
                principal_id: session.principal_id,
            },
        );
        Ok(())
    }

    async fn find_active_session(
        &self,
        token_hash: &SecretHash,
    ) -> Result<Option<AdminSession>, AdminStoreError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?
            .sessions
            .get(&SecretHashKey::from(token_hash))
            .copied())
    }

    async fn recent_failed_login_count(
        &self,
        account_key_hash: &SecretHash,
        source_ip_hash: &SecretHash,
    ) -> Result<u64, AdminStoreError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?
            .login_attempts
            .iter()
            .filter(|attempt| {
                attempt.outcome == AdminLoginOutcome::Failed
                    && &attempt.account_key_hash == account_key_hash
                    && &attempt.source_ip_hash == source_ip_hash
            })
            .count() as u64)
    }

    async fn revoke_session(
        &self,
        session_id: Uuid,
        _replacement_session_id: Option<Uuid>,
    ) -> Result<bool, AdminStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?;
        let key = state
            .sessions
            .iter()
            .find_map(|(key, session)| (session.id == session_id).then(|| key.clone()));
        Ok(key.is_some_and(|key| state.sessions.remove(&key).is_some()))
    }

    async fn rotate_session(
        &self,
        session_id: Uuid,
        replacement: NewAdminSession,
    ) -> Result<bool, AdminStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?;
        let old_key = state
            .sessions
            .iter()
            .find_map(|(key, session)| (session.id == session_id).then(|| key.clone()));
        let replacement_key = SecretHashKey::from(&replacement.token_hash);
        let old_session = old_key
            .as_ref()
            .and_then(|key| state.sessions.get(key))
            .copied();
        if old_session.is_none()
            || old_session.is_some_and(|session| session.principal_id != replacement.principal_id)
            || state.sessions.contains_key(&replacement_key)
        {
            return Ok(false);
        }
        state.sessions.remove(&old_key.expect("checked above"));
        state.sessions.insert(
            replacement_key,
            AdminSession {
                id: replacement.id,
                principal_id: replacement.principal_id,
            },
        );
        Ok(true)
    }

    async fn record_login_attempt(
        &self,
        attempt: AdminLoginAttempt,
    ) -> Result<(), AdminStoreError> {
        self.state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?
            .login_attempts
            .push(attempt);
        Ok(())
    }

    async fn record_security_event(
        &self,
        event: AdminSecurityEvent,
    ) -> Result<(), AdminStoreError> {
        self.state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?
            .security_events
            .push(event);
        Ok(())
    }

    async fn record_request(&self, request: AdminRequestRecord) -> Result<(), AdminStoreError> {
        self.state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?
            .request_records
            .push(request);
        Ok(())
    }

    async fn list_audit(
        &self,
        filter: AdminAuditFilter,
    ) -> Result<Vec<AdminAuditEntry>, AdminStoreError> {
        let state = self
            .state
            .lock()
            .map_err(|_| AdminStoreError::Unavailable)?;
        let mut entries = state
            .request_records
            .iter()
            .map(|record| AdminAuditEntry {
                occurred_at: String::new(),
                action: record.endpoint.to_owned(),
                outcome: record.outcome.as_str().to_owned(),
            })
            .collect::<Vec<_>>();
        entries.retain(|entry| {
            filter
                .action
                .as_ref()
                .is_none_or(|action| action == &entry.action)
                && filter
                    .outcome
                    .as_ref()
                    .is_none_or(|outcome| outcome == &entry.outcome)
        });
        Ok(entries
            .into_iter()
            .skip(filter.offset as usize)
            .take(filter.limit as usize)
            .collect())
    }
}
