//! Locations on tickets and assets (`ticket_locations`, `asset_locations`).
//!
//! Each write emits a `ticket_location.*` or `asset_location.*` sync record
//! that names the ticket or asset and nothing else: no address and no
//! coordinates reach the sync stream, so it holds no location to erase, and
//! the audit trigger records which parts changed without their values.
//! Clients fetch the row when the record arrives.

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{AssetLocation, LocationInput, SyncAggregate, SyncOp, Ticket, TicketLocation};
use crate::schema::{asset_locations, assets, ticket_locations, tickets};
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

/// Why a location can't be written.
#[derive(Debug, PartialEq)]
pub enum LocationError {
    /// Nothing to place: no text and no coordinates.
    Empty,
    /// Latitude without longitude, or the other way round.
    HalfCoordinates,
    /// Coordinates off the globe, or not finite.
    CoordinatesOutOfRange,
}

impl std::fmt::Display for LocationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LocationError::Empty => "a location needs an address or a pin",
            LocationError::HalfCoordinates => "latitude and longitude go together",
            LocationError::CoordinatesOutOfRange => "coordinates are off the map",
        })
    }
}

/// Text trimmed, with blanks as `None`.
fn clean(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// `input` with blank text dropped, checked against the table constraints so
/// a caller gets a reason rather than a database error.
pub fn normalise(input: LocationInput) -> Result<LocationInput, LocationError> {
    let out = LocationInput {
        label: clean(input.label),
        street: clean(input.street),
        city: clean(input.city),
        region: clean(input.region),
        postal_code: clean(input.postal_code),
        country: clean(input.country),
        geocode_source: clean(input.geocode_source),
        ..input
    };
    match (out.latitude, out.longitude) {
        (Some(lat), Some(lon)) => {
            // A range holds no NaN or infinity, so this refuses those too.
            if !((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)) {
                return Err(LocationError::CoordinatesOutOfRange);
            }
        }
        (None, None) => {
            let any_text = [
                &out.street,
                &out.city,
                &out.region,
                &out.postal_code,
                &out.country,
            ]
            .iter()
            .any(|v| v.is_some());
            if !any_text {
                return Err(LocationError::Empty);
            }
        }
        _ => return Err(LocationError::HalfCoordinates),
    }
    Ok(out)
}

/// A location write that failed: the input, or the database.
#[derive(Debug)]
pub enum WriteError {
    Invalid(LocationError),
    Db(diesel::result::Error),
}

impl From<diesel::result::Error> for WriteError {
    fn from(e: diesel::result::Error) -> Self {
        WriteError::Db(e)
    }
}

/// Builds the column tuple shared by both tables' insert and update.
macro_rules! location_values {
    ($t:ident, $i:expr) => {
        (
            $t::label.eq($i.label.clone()),
            $t::street.eq($i.street.clone()),
            $t::city.eq($i.city.clone()),
            $t::region.eq($i.region.clone()),
            $t::postal_code.eq($i.postal_code.clone()),
            $t::country.eq($i.country.clone()),
            $t::latitude.eq($i.latitude),
            $t::longitude.eq($i.longitude),
            $t::geocode_precision.eq($i.geocode_precision.map(|p| p.as_str())),
            $t::geocode_source.eq($i.geocode_source.clone()),
            $t::geocode_place_confirmed.eq($i.geocode_place_confirmed),
            $t::origin_type.eq($i.origin_type.map(|o| o.as_str())),
        )
    };
}

// ---- Tickets ---------------------------------------------------------------

/// The ticket's location, if it has one.
pub fn for_ticket(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Option<TicketLocation>> {
    ticket_locations::table
        .find(ticket_id)
        .select(TicketLocation::as_select())
        .first(conn)
        .optional()
}

/// The sync record for a change to `ticket`'s location: the ticket id alone.
fn ticket_record(
    conn: &mut DbConnection,
    ticket: &Ticket,
    op: SyncOp,
    event_type: &'static str,
) -> QueryResult<SyncEmit<'static>> {
    let groups = groups::for_ticket(conn, ticket)?;
    Ok(SyncEmit {
        aggregate: SyncAggregate::TicketLocation,
        aggregate_id: ticket.id.to_string(),
        op,
        event_type,
        data: json!({ "ticket_id": ticket.id }),
        groups,
        causation_id: None,
    })
}

/// Sets the ticket's location, replacing any it had. `actor` is recorded as
/// the creator of a new location.
pub fn set_for_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
    input: LocationInput,
    actor: Option<Uuid>,
) -> Result<TicketLocation, WriteError> {
    let input = normalise(input).map_err(WriteError::Invalid)?;
    let row = conn.transaction::<_, diesel::result::Error, _>(|conn| {
        let ticket: Ticket = tickets::table.find(ticket_id).first(conn)?;
        let existed = diesel::select(diesel::dsl::exists(ticket_locations::table.find(ticket_id)))
            .get_result::<bool>(conn)?;
        let row = diesel::insert_into(ticket_locations::table)
            .values((
                ticket_locations::ticket_id.eq(ticket_id),
                ticket_locations::created_by.eq(actor),
                location_values!(ticket_locations, input),
            ))
            .on_conflict(ticket_locations::ticket_id)
            .do_update()
            .set((
                location_values!(ticket_locations, input),
                ticket_locations::updated_at.eq(diesel::dsl::now),
            ))
            .returning(TicketLocation::as_returning())
            .get_result(conn)?;
        if existed {
            let record = ticket_record(conn, &ticket, SyncOp::Update, "ticket_location.updated")?;
            emit::record(conn, record)?;
        } else {
            let record = ticket_record(conn, &ticket, SyncOp::Insert, "ticket_location.added")?;
            emit::record(conn, record)?;
        }
        Ok(row)
    })?;
    Ok(row)
}

/// Removes the ticket's location. `false` when it had none.
pub fn clear_for_ticket(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<bool> {
    conn.transaction(|conn| {
        let ticket: Ticket = tickets::table.find(ticket_id).first(conn)?;
        let n = diesel::delete(ticket_locations::table.find(ticket_id)).execute(conn)?;
        if n > 0 {
            let record = ticket_record(conn, &ticket, SyncOp::Delete, "ticket_location.removed")?;
            emit::record(conn, record)?;
        }
        Ok(n > 0)
    })
}

// ---- Assets ----------------------------------------------------------------

/// The asset's structured location, if it has one.
pub fn for_asset(conn: &mut DbConnection, asset_id: i32) -> QueryResult<Option<AssetLocation>> {
    asset_locations::table
        .find(asset_id)
        .select(AssetLocation::as_select())
        .first(conn)
        .optional()
}

/// The sync record for a change to an asset's location: the asset id alone.
fn asset_record(asset_id: i32, op: SyncOp, event_type: &'static str) -> SyncEmit<'static> {
    SyncEmit {
        aggregate: SyncAggregate::AssetLocation,
        aggregate_id: asset_id.to_string(),
        op,
        event_type,
        data: json!({ "asset_id": asset_id }),
        groups: groups::workspace(),
        causation_id: None,
    }
}

/// Sets the asset's structured location, replacing any it had. The free
/// text in `assets.location` is left alone.
pub fn set_for_asset(
    conn: &mut DbConnection,
    asset_id: i32,
    input: LocationInput,
    actor: Option<Uuid>,
) -> Result<AssetLocation, WriteError> {
    let input = normalise(input).map_err(WriteError::Invalid)?;
    let row = conn.transaction::<_, diesel::result::Error, _>(|conn| {
        // Fails with NotFound when the asset isn't in this workspace.
        assets::table
            .find(asset_id)
            .select(assets::id)
            .first::<i32>(conn)?;
        let existed = diesel::select(diesel::dsl::exists(asset_locations::table.find(asset_id)))
            .get_result::<bool>(conn)?;
        let row = diesel::insert_into(asset_locations::table)
            .values((
                asset_locations::asset_id.eq(asset_id),
                asset_locations::created_by.eq(actor),
                location_values!(asset_locations, input),
            ))
            .on_conflict(asset_locations::asset_id)
            .do_update()
            .set((
                location_values!(asset_locations, input),
                asset_locations::updated_at.eq(diesel::dsl::now),
            ))
            .returning(AssetLocation::as_returning())
            .get_result(conn)?;
        if existed {
            emit::record(
                conn,
                asset_record(asset_id, SyncOp::Update, "asset_location.updated"),
            )?;
        } else {
            emit::record(
                conn,
                asset_record(asset_id, SyncOp::Insert, "asset_location.added"),
            )?;
        }
        Ok(row)
    })?;
    Ok(row)
}

/// Removes the asset's structured location. `false` when it had none.
pub fn clear_for_asset(conn: &mut DbConnection, asset_id: i32) -> QueryResult<bool> {
    conn.transaction(|conn| {
        let n = diesel::delete(asset_locations::table.find(asset_id)).execute(conn)?;
        if n > 0 {
            emit::record(
                conn,
                asset_record(asset_id, SyncOp::Delete, "asset_location.removed"),
            )?;
        }
        Ok(n > 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::GeocodePrecision;

    fn at(lat: f64, lon: f64) -> LocationInput {
        LocationInput {
            latitude: Some(lat),
            longitude: Some(lon),
            ..Default::default()
        }
    }

    #[test]
    fn a_pin_alone_is_a_location() {
        assert!(normalise(at(-37.81, 144.96)).is_ok());
    }

    #[test]
    fn text_alone_is_a_location() {
        let input = LocationInput {
            city: Some(" Windsor ".into()),
            ..Default::default()
        };
        assert_eq!(normalise(input).unwrap().city.as_deref(), Some("Windsor"));
    }

    #[test]
    fn blanks_are_nothing() {
        let input = LocationInput {
            street: Some("  ".into()),
            label: Some(String::new()),
            ..Default::default()
        };
        assert_eq!(normalise(input), Err(LocationError::Empty));
    }

    #[test]
    fn coordinates_come_in_pairs() {
        let input = LocationInput {
            latitude: Some(1.0),
            city: Some("Perth".into()),
            ..Default::default()
        };
        assert_eq!(normalise(input), Err(LocationError::HalfCoordinates));
    }

    #[test]
    fn coordinates_stay_on_the_globe() {
        for (lat, lon) in [
            (91.0, 0.0),
            (0.0, -180.5),
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
        ] {
            assert_eq!(
                normalise(at(lat, lon)),
                Err(LocationError::CoordinatesOutOfRange),
                "{lat} {lon}"
            );
        }
    }

    #[test]
    fn precision_and_origin_keep_their_names() {
        let input = LocationInput {
            geocode_precision: Some(GeocodePrecision::Locality),
            ..at(0.0, 0.0)
        };
        assert_eq!(
            normalise(input)
                .unwrap()
                .geocode_precision
                .map(|p| p.as_str()),
            Some("locality")
        );
    }
}
