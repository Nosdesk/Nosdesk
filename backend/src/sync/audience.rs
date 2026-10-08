//! Who each kind of sync record is for.
//!
//! Every [`SyncAggregate`] states its audience in [`audience`], a match with
//! no wildcard: a new kind of record doesn't compile until someone decides
//! who receives it. Each `backend/sync-models/<name>.json` repeats the answer
//! as `"audience"`, and `tests/it/sync_model_registry.rs` holds the two to the
//! same value. [`crate::sync::visibility`] applies it on all three read paths
//! (bootstrap, delta, the live stream).

use crate::models::SyncAggregate;

/// Who receives a kind of sync record, within the workspace whose feed it is
/// in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// Everyone in the workspace, requester-role members included.
    All,
    /// Staff: workspace agents, admins and owners, and platform admins.
    Staff,
    /// Workspace admins and owners, and platform admins.
    Admin,
    /// Whoever can see the ticket the record belongs to.
    Ticket,
    /// Whoever the documentation page or collection is open to.
    Docs,
    /// Only the person the record is addressed to: the `user:<uuid>` group
    /// it was written with.
    Addressee,
    /// Staff, and the person the record is addressed to.
    StaffAndAddressee,
}

impl Audience {
    /// The name `sync-models/*.json` use for it.
    pub fn as_str(self) -> &'static str {
        match self {
            Audience::All => "all",
            Audience::Staff => "staff",
            Audience::Admin => "admin",
            Audience::Ticket => "ticket",
            Audience::Docs => "docs",
            Audience::Addressee => "self",
            Audience::StaffAndAddressee => "staff+self",
        }
    }
}

/// The audience of `aggregate`'s records.
pub fn audience(aggregate: SyncAggregate) -> Audience {
    match aggregate {
        // Read by every ticket view and the asset, project and cycle routes,
        // which members reach too.
        SyncAggregate::Asset => Audience::All,
        SyncAggregate::Cycle => Audience::All,
        SyncAggregate::Project => Audience::All,
        SyncAggregate::WorkflowState => Audience::All,
        // Plugins load in every signed-in session and tear down on a
        // `plugin.*` record.
        SyncAggregate::Plugin => Audience::All,
        // Everyone gets the workspace's people, but only some of each row:
        // see `sync::user_projection`.
        SyncAggregate::User => Audience::All,

        SyncAggregate::AssetAudit => Audience::Staff,
        SyncAggregate::AssetLifecycleEvent => Audience::Staff,
        SyncAggregate::AssetMedia => Audience::Staff,
        // Nothing records it yet; staff until something does.
        SyncAggregate::Assignment => Audience::Staff,
        SyncAggregate::GroupMembership => Audience::Staff,
        // Staff, less a gap naming a page the viewer can't open.
        SyncAggregate::KnowledgeGap => Audience::Staff,
        SyncAggregate::TicketReference => Audience::Staff,

        SyncAggregate::Channel => Audience::Admin,
        SyncAggregate::Data => Audience::Admin,
        SyncAggregate::Webhook => Audience::Admin,

        SyncAggregate::Ticket => Audience::Ticket,
        SyncAggregate::Comment => Audience::Ticket,
        // On a comment, the comment's ticket; a draft upload not yet on one
        // goes to its uploader alone.
        SyncAggregate::Attachment => Audience::Ticket,
        SyncAggregate::TicketAsset => Audience::Ticket,
        SyncAggregate::LinkedTicket => Audience::Ticket,
        SyncAggregate::ProjectTicket => Audience::Ticket,
        SyncAggregate::CycleTicket => Audience::Ticket,
        // Usage recorded against a ticket; restocks and write-offs are staff
        // only.
        SyncAggregate::AssetUsage => Audience::Ticket,

        SyncAggregate::DocumentationPage => Audience::Docs,
        SyncAggregate::DocumentationCollection => Audience::Docs,

        SyncAggregate::Notification => Audience::Addressee,
        // The borrower follows their own loan.
        SyncAggregate::AssetLoan => Audience::StaffAndAddressee,
    }
}
