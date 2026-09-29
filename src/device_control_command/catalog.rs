//! Station catalog resolution and stream preparation for playback commands.

use std::time::Duration;

use async_trait::async_trait;

use crate::{
    device_control::{StationPresentation, validate_stream_uri},
    search::{RepositoryError, SearchService},
};

/// Budget for one server-side station resolution inside command admission.
pub const RESOLUTION_TIMEOUT: Duration = Duration::from_secs(5);

/// Bounded server-resolved station data sent only to the selected target.
///
/// The stream URI is never persisted, logged, or sent to a controller. Presentation is a
/// target-local display hint and does not replace the stable station identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedStation {
    /// Validated playable stream URL for the target's one-time delivery.
    pub stream_uri: String,
    /// Display values for a target whose local catalog lacks the resolved station.
    pub presentation: StationPresentation,
}

/// Read-only station catalog boundary used to resolve station IDs for playback delivery.
#[async_trait]
pub trait StationCatalog: Send + Sync {
    /// Returns one current playable station, or `None` when its stable ID is unknown.
    async fn station(&self, station_id: &str) -> Result<Option<ResolvedStation>, RepositoryError>;
}

#[async_trait]
impl StationCatalog for SearchService {
    async fn station(&self, station_id: &str) -> Result<Option<ResolvedStation>, RepositoryError> {
        Ok(self
            .public_station(station_id)
            .await?
            .map(|station| ResolvedStation {
                stream_uri: station.stream_url,
                presentation: StationPresentation {
                    name: station.name,
                    // The catalog has no stored icon yet. Keeping the null slot in the command
                    // avoids a future wire-contract change when that catalog field is added.
                    icon_url: None,
                },
            }))
    }
}

/// Outcome of one server-side station resolution attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionFailure {
    /// Deterministic admission failure with a fixed message that never echoes identifiers.
    Invalid(&'static str),
    /// Transient catalog backend failure; retryable with a fresh command.
    Unavailable,
}

/// Resolves one controller station reference into a validated target delivery.
///
/// Deterministic failures (unknown station, unusable stream, forbidden destination) return a
/// fixed `invalid_payload` message; catalog errors and resolution timeouts are transient.
pub async fn resolve_station(
    catalog: &dyn StationCatalog,
    station_id: &str,
) -> Result<ResolvedStation, ResolutionFailure> {
    let station = match tokio::time::timeout(RESOLUTION_TIMEOUT, catalog.station(station_id)).await
    {
        Ok(Ok(Some(station))) => station,
        Ok(Ok(None)) => {
            return Err(ResolutionFailure::Invalid("Unknown station identifier."));
        }
        Ok(Err(_)) | Err(_) => return Err(ResolutionFailure::Unavailable),
    };
    if station.stream_uri.trim().is_empty() {
        return Err(ResolutionFailure::Invalid(
            "Station has no playable stream.",
        ));
    }
    if station.presentation.validate().is_err() {
        return Err(ResolutionFailure::Invalid(
            "Station presentation is not allowed.",
        ));
    }
    match validate_stream_uri(&station.stream_uri) {
        Ok(()) => Ok(station),
        Err(_) => Err(ResolutionFailure::Invalid("Stream address is not allowed.")),
    }
}
