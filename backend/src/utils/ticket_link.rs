//! Links to a ticket in the web apps, built in one place.
//!
//! People know a ticket by its number, and the agent app and the portal both
//! route `/tickets/{number}`. A link to a ticket known only by its id goes to
//! `/tickets/id/{id}`, where the agent app looks the number up and redirects:
//! an id never lands where a number belongs. Every ticket link the server
//! builds comes from here; `tests/it/ticket_url_lint.rs` refuses a `/tickets/`
//! route formatted anywhere else.

/// `/tickets/{number}`.
pub fn ticket_route(number: i32) -> String {
    format!("/tickets/{number}")
}

/// `/tickets/id/{id}`, for a ticket known only by its id.
pub fn ticket_route_by_id(id: i32) -> String {
    format!("/tickets/id/{id}")
}

/// The route by number when it is known, else by id.
pub fn ticket_route_for(id: i32, number: Option<i32>) -> String {
    number.map_or_else(|| ticket_route_by_id(id), ticket_route)
}

/// The ticket id in a search index document's route: `/tickets/id/{id}`, or
/// `/tickets/{id}` in a document indexed before index routes said `id`.
pub fn ticket_id_in_index_route(url: &str) -> Option<i32> {
    url.strip_prefix("/tickets/id/")
        .or_else(|| url.strip_prefix("/tickets/"))?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        assert_eq!(ticket_route(7), "/tickets/7");
        assert_eq!(ticket_route_by_id(42), "/tickets/id/42");
        assert_eq!(ticket_route_for(42, Some(7)), "/tickets/7");
        assert_eq!(ticket_route_for(42, None), "/tickets/id/42");
    }

    #[test]
    fn index_routes_name_the_id() {
        assert_eq!(ticket_id_in_index_route("/tickets/id/42"), Some(42));
        assert_eq!(ticket_id_in_index_route("/tickets/42"), Some(42));
        assert_eq!(ticket_id_in_index_route("/assets/42"), None);
        assert_eq!(ticket_id_in_index_route("/tickets/id/x"), None);
    }
}
