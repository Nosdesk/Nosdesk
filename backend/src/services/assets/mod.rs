//! Asset domain services. Today only the JSON-Schema-subset
//! validator that gates per-kind attribute writes lives here;
//! future asset-level concerns (consumable usage tally,
//! quantity guards) belong alongside.

pub mod kinds;
