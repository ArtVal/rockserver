//! Passkey-only account, device, and native-session domain boundaries.

pub mod webauthn;

use std::fmt;

use uuid::Uuid;

/// Maximum number of concurrently active native devices per Rock account.
pub const MAX_ACCOUNT_DEVICES: u8 = 50;

/// A fixed-size keyed digest of a bearer secret, safe to persist but deliberately opaque in logs.
#[derive(Clone, Eq, PartialEq)]
pub struct SecretHash([u8; 32]);

impl SecretHash {
    /// Constructs a hash from a trusted token-hashing boundary; raw tokens are never accepted here.
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// The WebAuthn ceremony being validated by the server boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebAuthnCeremony {
    /// A new passkey registration ceremony.
    Registration,
    /// An existing passkey assertion ceremony.
    Authentication,
}

impl WebAuthnCeremony {
    /// Returns the WebAuthn client-data type required for this ceremony.
    pub const fn client_data_type(self) -> &'static str {
        match self {
            Self::Registration => "webauthn.create",
            Self::Authentication => "webauthn.get",
        }
    }
}

/// Untrusted WebAuthn client-data fields checked before cryptographic verification.
pub struct WebAuthnClientData<'a> {
    /// Ceremony selected by the server.
    pub ceremony: WebAuthnCeremony,
    /// Base64url challenge returned by the authenticator.
    pub challenge: &'a str,
    /// Challenge issued for this single-use ceremony.
    pub expected_challenge: &'a str,
    /// Browser origin reported by the WebAuthn client.
    pub origin: &'a str,
    /// Configured first-party origin.
    pub expected_origin: &'a str,
    /// Relying-party identifier selected by the server.
    pub rp_id: &'a str,
    /// Configured relying-party identifier.
    pub expected_rp_id: &'a str,
    /// WebAuthn client-data type.
    pub client_data_type: &'a str,
}

/// Safe, non-sensitive WebAuthn validation failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebAuthnValidationError {
    /// A challenge, origin, RP ID, or client-data type did not match policy.
    ClientDataMismatch,
    /// The authenticator sign counter moved backwards under the approved clone policy.
    SignCountRollback,
}

/// Validates origin, RP ID, challenge, and ceremony type before signature verification.
pub fn validate_webauthn_client_data(
    data: &WebAuthnClientData<'_>,
) -> Result<(), WebAuthnValidationError> {
    if data.challenge.is_empty()
        || data.challenge != data.expected_challenge
        || data.origin != data.expected_origin
        || data.rp_id != data.expected_rp_id
        || data.client_data_type != data.ceremony.client_data_type()
    {
        return Err(WebAuthnValidationError::ClientDataMismatch);
    }
    Ok(())
}

/// Applies the approved sign-counter policy; zero remains valid for authenticators without counters.
pub fn validate_sign_count(previous: i64, current: i64) -> Result<(), WebAuthnValidationError> {
    if previous < 0 || current < 0 || (previous > 0 && current == 0) || current < previous {
        return Err(WebAuthnValidationError::SignCountRollback);
    }
    Ok(())
}

impl fmt::Debug for SecretHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretHash([REDACTED])")
    }
}

/// Identifies an active device only within its owning account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedDevice {
    /// Stable public device identifier.
    pub id: Uuid,
    /// Owning account identifier.
    pub user_id: Uuid,
    /// User-visible bounded device name.
    pub device_display_name: String,
    /// Stable client type supplied by the native client.
    pub device_type: String,
    /// Device creation time in RFC 3339 UTC form when read from persistence.
    pub created_at: String,
    /// Last activity time in RFC 3339 UTC form when read from persistence.
    pub last_seen_at: Option<String>,
}

/// Non-secret device projection for the first-party browser account centre.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserDevice {
    /// Opaque identifier used only by the browser's protected management calls.
    pub id: Uuid,
    /// Human-readable name selected for the device.
    pub device_display_name: String,
    /// Stable client family supplied during pairing.
    pub device_type: String,
    /// Time the device was connected.
    pub created_at: String,
    /// Most recent recorded native activity, when available.
    pub last_seen_at: Option<String>,
    /// Safe aggregate status derived from active native sessions.
    pub session_status: String,
}

/// Active native session owner resolved from a hashed access token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActiveSession {
    /// Native session identifier.
    pub session_id: Uuid,
    /// Owning account identifier.
    pub user_id: Uuid,
    /// Device that owns the session.
    pub device_id: Uuid,
}

/// Failure to resolve an active native session without exposing storage internals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeSessionLookupError;

/// Resolves an opaque native access-token hash to its active server-owned session.
///
/// Implementations must reject expired, revoked, inactive, and ownership-inconsistent sessions.
#[async_trait::async_trait]
pub trait NativeSessionResolver: Send + Sync {
    /// Resolves one active native session, or reports a retryable lookup failure.
    async fn resolve_active_native_session(
        &self,
        access_hash: &SecretHash,
    ) -> Result<Option<ActiveSession>, NativeSessionLookupError>;
}

/// Hashed material required to create one short-lived native access session.
pub struct NewSession<'a> {
    /// New session identifier.
    pub session_id: Uuid,
    /// Account that owns the session.
    pub user_id: Uuid,
    /// Active device that owns the session.
    pub device_id: Uuid,
    /// Keyed hash of the opaque access token.
    pub access_hash: &'a SecretHash,
    /// Access expiry in PostgreSQL-validated RFC 3339 UTC form.
    pub access_expires_at_rfc3339: &'a str,
}

/// Hashed material for a paired device whose account owner is derived from an approved pairing request.
pub struct NewPairingSession<'a> {
    /// New native session identifier.
    pub session_id: Uuid,
    /// New device identifier selected by the server.
    pub device_id: Uuid,
    /// Keyed hash of the opaque access token.
    pub access_hash: &'a SecretHash,
    /// Access expiry in PostgreSQL-validated RFC 3339 UTC form.
    pub access_expires_at_rfc3339: &'a str,
    /// Keyed hash of the persistent opaque device credential.
    pub device_secret_hash: &'a SecretHash,
}

/// Sliding idle window of a browser session: authenticated traffic keeps
/// renewing `expires_at` up to this many minutes, so an active user stays
/// signed in like on other consumer sites.
pub const BROWSER_SESSION_IDLE_MINUTES: i32 = 30 * 24 * 60;
/// Absolute browser-session lifetime counted from the passkey sign-in that
/// created it; reaching it always requires a fresh passkey ceremony.
pub const BROWSER_SESSION_ABSOLUTE_MINUTES: i32 = 180 * 24 * 60;

/// Hashed browser-session material created after a passkey ceremony.
pub struct NewBrowserSession<'a> {
    /// Browser session identifier.
    pub session_id: Uuid,
    /// Hash of the opaque browser-session cookie value.
    pub session_token_hash: &'a SecretHash,
    /// Owning account identifier.
    pub user_id: Uuid,
    /// Hash of the CSRF token bound to the browser cookie.
    pub csrf_hash: &'a SecretHash,
    /// Time of the last fresh passkey assertion in RFC 3339 UTC form.
    pub passkey_reauthenticated_at_rfc3339: &'a str,
    /// Browser-session expiry in RFC 3339 UTC form.
    pub expires_at_rfc3339: &'a str,
    /// Hard expiry the sliding window can never extend past, in RFC 3339 UTC form.
    pub absolute_expires_at_rfc3339: &'a str,
}

/// Hashed passkey, challenge, and browser-session material committed as one account transaction.
pub struct NewPasskeyRegistration<'a> {
    /// Account identifier reserved by the server for this ceremony.
    pub user_id: Uuid,
    /// New account's user-facing label.
    pub account_display_name: &'a str,
    /// Registration challenge row to consume.
    pub challenge_id: Uuid,
    /// Hash of the challenge value stored in the server-side state.
    pub challenge_hash: &'a SecretHash,
    /// New passkey row identifier.
    pub credential_id: Uuid,
    /// Authenticator credential identifier bytes.
    pub credential_bytes: &'a [u8],
    /// COSE public key bytes.
    pub public_key: &'a [u8],
    /// Authenticator sign counter at registration.
    pub sign_count: i64,
    /// Authenticator transport hints.
    pub transports: &'a [String],
    /// Browser session issued after successful registration.
    pub browser_session: NewBrowserSession<'a>,
}

/// Hashed proofs and bounded device metadata for a pending desktop pairing request.
pub struct NewPairingRequest<'a> {
    /// Pairing request identifier.
    pub request_id: Uuid,
    /// Hash of the desktop-only completion proof.
    pub desktop_token_hash: &'a SecretHash,
    /// Hash of the QR approval proof.
    pub approval_secret_hash: &'a SecretHash,
    /// Hash of the short-code lookup value.
    pub short_code_hash: &'a SecretHash,
    /// Human-visible phrase shown by both devices.
    pub verification_phrase: &'a str,
    /// Suggested user-visible device name.
    pub device_display_name: &'a str,
    /// Target client type.
    pub device_type: &'a str,
    /// Optional client version.
    pub app_version: Option<&'a str>,
    /// Expiry in RFC 3339 UTC form.
    pub expires_at_rfc3339: &'a str,
}

/// Bounded metadata persisted for a WebAuthn challenge.
pub struct NewWebAuthnChallenge<'a> {
    /// Challenge record identifier.
    pub challenge_id: Uuid,
    /// Hash of the single-use challenge value.
    pub challenge_hash: &'a SecretHash,
    /// Serialized server-only passkey ceremony state.
    pub state_blob: &'a [u8],
    /// Ceremony this challenge belongs to.
    pub ceremony: WebAuthnCeremony,
    /// Expected RP ID.
    pub rp_id: &'a str,
    /// Expected first-party origin.
    pub origin: &'a str,
    /// Challenge expiry in RFC 3339 UTC form.
    pub expires_at_rfc3339: &'a str,
    /// Optional account associated with authentication/registration continuation.
    pub user_id: Option<Uuid>,
    /// Optional browser session associated with the challenge.
    pub browser_session_id: Option<Uuid>,
    /// Optional pairing request continued by the browser.
    pub pairing_request_id: Option<Uuid>,
}

/// Identifiers returned after an approved pairing atomically creates a device and session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairingCompletion {
    /// Account bound to the approved request.
    pub user_id: Uuid,
    /// Newly created desktop device.
    pub device_id: Uuid,
    /// Newly created native session.
    pub session_id: Uuid,
    /// Access expiry in PostgreSQL-formatted RFC 3339 UTC.
    pub access_expires_at: String,
    /// User-visible owner account name resolved from the approved browser session.
    pub account_display_name: String,
    /// User-visible name of the newly connected device.
    pub device_display_name: String,
    /// Stable type of the newly connected device.
    pub device_type: String,
}

/// Safe persistence outcome for a native pairing completion attempt.
#[derive(Debug, Eq, PartialEq)]
pub enum PairingCompletionOutcome {
    /// The request is valid but browser approval has not happened yet.
    Pending,
    /// The request is no longer usable because it expired, was consumed, or was revoked.
    NoLongerAvailable,
    /// The account has reached its active-device limit.
    DeviceLimit,
    /// The supplied desktop proof did not identify a valid request.
    InvalidProof,
    /// The request was consumed and the device/session were created.
    Completed(PairingCompletion),
}

/// Safe persistence outcome for a first-account passkey registration attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasskeyRegistrationOutcome {
    /// The account, credential, browser session, and challenge consumption committed together.
    Created,
    /// The server-side registration challenge is no longer valid.
    ChallengeRejected,
    /// The authenticator credential is already registered.
    CredentialAlreadyRegistered,
}

/// Non-secret pairing details shown to a browser after a short-code lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairingPreview {
    /// Pending pairing request identifier.
    pub request_id: Uuid,
    /// Suggested user-visible device name.
    pub device_display_name: String,
    /// Target client type.
    pub device_type: String,
    /// Optional client version.
    pub app_version: Option<String>,
    /// Human-visible phrase that must be confirmed by the user.
    pub verification_phrase: String,
    /// Database-formatted UTC expiry for the pending request.
    pub expires_at: String,
}

/// User-facing account and current-device metadata resolved only for an authenticated session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountProjection {
    /// User-visible account name.
    pub account_display_name: String,
    /// Account creation time in RFC 3339 UTC form.
    pub created_at: String,
    /// Current device display name.
    pub device_display_name: String,
    /// Current device type.
    pub device_type: String,
    /// Current device creation time in RFC 3339 UTC form.
    pub device_created_at: String,
    /// Current device last activity time in RFC 3339 UTC form, when recorded.
    pub last_seen_at: Option<String>,
}

/// Recognizes the small audited security-event vocabulary allowed by this persistence stage.
pub fn is_safe_audit_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "account_registered"
            | "passkey_registered"
            | "passkey_revoked"
            | "pairing_completed"
            | "device_session_issued"
            | "refresh_rotated"
            | "refresh_reuse_detected"
            | "logout"
            | "browser_logout"
            | "device_revoked"
            | "device_renamed"
            | "account_deletion_accepted"
            | "account_deletion_completed"
            | "operator_account_deactivated"
            | "operator_device_revoked"
            | "operator_passkey_revoked"
            | "yandex_home_connected"
            | "yandex_home_disconnected"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_hash_debug_is_redacted() {
        assert_eq!(
            format!("{:?}", SecretHash::new([7; 32])),
            "SecretHash([REDACTED])"
        );
    }

    #[test]
    fn audit_vocabulary_rejects_request_secrets() {
        assert!(is_safe_audit_event("device_session_issued"));
        assert!(!is_safe_audit_event("refresh_token=secret"));
    }

    #[test]
    fn webauthn_client_data_requires_exact_server_context() {
        let valid = WebAuthnClientData {
            ceremony: WebAuthnCeremony::Authentication,
            challenge: "challenge",
            expected_challenge: "challenge",
            origin: super::webauthn::ORIGIN,
            expected_origin: super::webauthn::ORIGIN,
            rp_id: super::webauthn::RP_ID,
            expected_rp_id: super::webauthn::RP_ID,
            client_data_type: "webauthn.get",
        };
        assert_eq!(validate_webauthn_client_data(&valid), Ok(()));
        let wrong_origin = WebAuthnClientData {
            ceremony: WebAuthnCeremony::Authentication,
            challenge: "challenge",
            expected_challenge: "challenge",
            origin: "https://evil.example",
            expected_origin: super::webauthn::ORIGIN,
            rp_id: super::webauthn::RP_ID,
            expected_rp_id: super::webauthn::RP_ID,
            client_data_type: "webauthn.get",
        };
        assert_eq!(
            validate_webauthn_client_data(&wrong_origin),
            Err(WebAuthnValidationError::ClientDataMismatch)
        );
        let mut wrong_rp = valid;
        wrong_rp.rp_id = "evil.example";
        assert_eq!(
            validate_webauthn_client_data(&wrong_rp),
            Err(WebAuthnValidationError::ClientDataMismatch)
        );
    }

    #[test]
    fn sign_count_policy_detects_clone_rollback() {
        assert_eq!(validate_sign_count(0, 0), Ok(()));
        assert_eq!(validate_sign_count(7, 8), Ok(()));
        assert_eq!(
            validate_sign_count(7, 6),
            Err(WebAuthnValidationError::SignCountRollback)
        );
        assert_eq!(
            validate_sign_count(7, 0),
            Err(WebAuthnValidationError::SignCountRollback)
        );
    }
}
