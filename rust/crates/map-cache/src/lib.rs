//! Offline-map policy and region metadata. Downloads are only valid for opted-in providers.
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileProvider {
    pub name: String,
    pub tile_url_template: String,
    pub attribution: String,
    pub offline_download_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineRegion {
    pub name: String,
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
    pub min_zoom: u8,
    pub max_zoom: u8,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MapPolicyError {
    #[error("provider '{0}' does not permit offline downloads")]
    OfflineDownloadForbidden(String),
    #[error("invalid map region")]
    InvalidRegion,
}

pub fn validate_download(
    provider: &TileProvider,
    region: &OfflineRegion,
) -> Result<(), MapPolicyError> {
    if !provider.offline_download_allowed {
        return Err(MapPolicyError::OfflineDownloadForbidden(
            provider.name.clone(),
        ));
    }
    if region.min_lat >= region.max_lat
        || region.min_lon >= region.max_lon
        || region.min_zoom > region.max_zoom
    {
        return Err(MapPolicyError::InvalidRegion);
    }
    Ok(())
}

/// The OSM public raster server must never be used as an offline provider.
pub fn osm_public_provider() -> TileProvider {
    TileProvider {
        name: "OpenStreetMap public raster tiles".into(),
        tile_url_template: "https://tile.openstreetmap.org/{z}/{x}/{y}.png".into(),
        attribution: "© OpenStreetMap contributors".into(),
        offline_download_allowed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocks_public_osm_prefetch() {
        let r = OfflineRegion {
            name: "field".into(),
            min_lat: 0.0,
            min_lon: 0.0,
            max_lat: 1.0,
            max_lon: 1.0,
            min_zoom: 10,
            max_zoom: 12,
        };
        assert!(matches!(
            validate_download(&osm_public_provider(), &r),
            Err(MapPolicyError::OfflineDownloadForbidden(_))
        ));
    }
}
