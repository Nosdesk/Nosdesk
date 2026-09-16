use diesel::prelude::*;

// ============================================================================
// Search Query Log (Phase 2c of the docs/KB redesign)
// ============================================================================

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::search_query_log)]
pub struct NewSearchQueryLog {
    pub query_raw: String,
    pub query_norm: String,
    pub result_count: i32,
}
