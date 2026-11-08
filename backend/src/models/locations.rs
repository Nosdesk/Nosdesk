//! Locations on tickets and assets (`ticket_locations`, `asset_locations`).
//!
//! A ticket's location is a copy of an address, never a reference to one, so
//! it stays where the job was when the requester moves. Both tables share one
//! shape; [`LocationInput`] writes either.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::{asset_locations, ticket_locations};

/// How close a geocode got. Stored as text; mirrors `woopwoop::Precision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeocodePrecision {
    House,
    Street,
    Locality,
}

impl GeocodePrecision {
    pub fn as_str(self) -> &'static str {
        match self {
            GeocodePrecision::House => "house",
            GeocodePrecision::Street => "street",
            GeocodePrecision::Locality => "locality",
        }
    }
}

/// `geocode_source` for a pin someone placed by hand.
pub const MANUAL_SOURCE: &str = "manual";

/// The type of the address a location was copied from. `None` on a location
/// an agent searched for or pinned. Erasure reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationOrigin {
    Work,
    Home,
    Other,
}

impl LocationOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            LocationOrigin::Work => "work",
            LocationOrigin::Home => "home",
            LocationOrigin::Other => "other",
        }
    }

    /// The origin for a copied `user_addresses.address_type`.
    pub fn from_address_type(address_type: &str) -> Option<Self> {
        match address_type {
            "work" => Some(LocationOrigin::Work),
            "home" => Some(LocationOrigin::Home),
            "other" => Some(LocationOrigin::Other),
            _ => None,
        }
    }
}

/// A location to write on a ticket or an asset.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct LocationInput {
    pub label: Option<String>,
    pub street: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub geocode_precision: Option<GeocodePrecision>,
    pub geocode_source: Option<String>,
    pub geocode_place_confirmed: Option<bool>,
    pub origin_type: Option<LocationOrigin>,
}

/// A ticket's location.
#[derive(Debug, Clone, Queryable, Selectable, Serialize)]
#[diesel(table_name = ticket_locations)]
pub struct TicketLocation {
    pub ticket_id: i32,
    pub label: Option<String>,
    pub street: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub geocode_precision: Option<String>,
    pub geocode_source: Option<String>,
    pub geocode_place_confirmed: Option<bool>,
    pub origin_type: Option<String>,
    pub created_by: Option<Uuid>,
    #[serde(skip)]
    pub workspace_id: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An asset's structured location, beside the free text in `assets.location`.
#[derive(Debug, Clone, Queryable, Selectable, Serialize)]
#[diesel(table_name = asset_locations)]
pub struct AssetLocation {
    pub asset_id: i32,
    pub label: Option<String>,
    pub street: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub geocode_precision: Option<String>,
    pub geocode_source: Option<String>,
    pub geocode_place_confirmed: Option<bool>,
    pub origin_type: Option<String>,
    pub created_by: Option<Uuid>,
    #[serde(skip)]
    pub workspace_id: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
