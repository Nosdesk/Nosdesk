//! The asset status rules: which category a new asset may start in, and
//! which moves between categories are allowed.
//!
//! The nine [`AssetStatus`] keys are fixed categories. Every write of
//! `assets.status` asks this module first: a lifecycle transition (by hand,
//! a rule step, an import, a replacement's carry-over), the loan ledger's
//! issue and return, and asset create. Nothing here reads the database, so a
//! dry run, a rule's shadow mode and the status dialog's target list (`GET
//! /assets/statuses`) get the same answer the write gets.
//!
//! The matrix is data ([`Matrix`]); [`FIXED`] is the one every path uses. A
//! workspace layer can narrow it later by wrapping [`FIXED`] and refusing
//! more cells, never by allowing one it refuses.

use serde::Serialize;

use crate::models::AssetStatus;
use crate::utils::i18n;

/// Why a move or a start is refused. The code is stable: the API answers
/// with it and clients branch on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    /// The asset is already in the target category. Bulk callers read it as
    /// unchanged, not as a failure.
    SameStatus,
    /// `disposed` is final.
    Terminal,
    /// The asset has an active loan; only returning the loan moves it off
    /// `on_loan`.
    LoanOwned,
    /// `on_loan` comes from issuing a loan; by hand it only restores an asset
    /// whose loan is still active.
    NeedsActiveLoan,
    /// A loan starts only from a loanable category.
    NotLoanable,
    /// A loan is returned only from `on_loan`.
    NotOnLoan,
    /// A move into `disposed` carries its disposal record.
    NeedsDisposal,
    /// A disposal record goes with a move into `disposed` and nothing else.
    DisposalNotAllowed,
    /// The category is where an asset starts; nothing moves back into it.
    StartOnly,
    /// A new asset can't start on loan; it is lent once it exists.
    StartOnLoan,
}

impl RefusalCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SameStatus => "same_status",
            Self::Terminal => "terminal",
            Self::LoanOwned => "loan_owned",
            Self::NeedsActiveLoan => "needs_active_loan",
            Self::NotLoanable => "not_loanable",
            Self::NotOnLoan => "not_on_loan",
            Self::NeedsDisposal => "needs_disposal",
            Self::DisposalNotAllowed => "disposal_not_allowed",
            Self::StartOnly => "start_only",
            Self::StartOnLoan => "start_on_loan",
        }
    }

    /// The catalogue key of the refusal's message.
    pub fn message_key(self) -> &'static str {
        match self {
            Self::SameStatus => "asset-move-refused-same-status",
            Self::Terminal => "asset-move-refused-terminal",
            Self::LoanOwned => "asset-move-refused-loan-owned",
            Self::NeedsActiveLoan => "asset-move-refused-needs-active-loan",
            Self::NotLoanable => "asset-move-refused-not-loanable",
            Self::NotOnLoan => "asset-move-refused-not-on-loan",
            Self::NeedsDisposal => "asset-move-refused-needs-disposal",
            Self::DisposalNotAllowed => "asset-move-refused-disposal-not-allowed",
            Self::StartOnly => "asset-move-refused-start-only",
            Self::StartOnLoan => "asset-move-refused-start-on-loan",
        }
    }
}

/// A refused move or start: its code, and the categories its message names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveRefused {
    pub code: RefusalCode,
    /// The category the asset is in; `None` for a new asset.
    pub from: Option<AssetStatus>,
    pub to: AssetStatus,
}

impl MoveRefused {
    pub fn is_same_status(&self) -> bool {
        self.code == RefusalCode::SameStatus
    }

    /// The refusal in words, in en-US. Imports and rule runs store it, so it
    /// reads the same at the dry run, the commit and every later read.
    pub fn message(&self) -> String {
        let locale = i18n_default_locale();
        let label = |status: AssetStatus| i18n::tr(&locale, &category_label_key(status));
        let status = match self.code {
            RefusalCode::NotLoanable | RefusalCode::NotOnLoan => self.from.unwrap_or(self.to),
            _ => self.to,
        };
        i18n::tr_with(
            &locale,
            self.code.message_key(),
            &[("status", label(status).into())],
        )
    }
}

fn i18n_default_locale() -> unic_langid::LanguageIdentifier {
    crate::utils::locale::effective_locale(None, crate::utils::locale::DEFAULT_LOCALE)
}

/// The catalogue key of a category's name, `asset-status-in-service` and so on.
pub fn category_label_key(status: AssetStatus) -> String {
    format!("asset-status-{}", status.as_str().replace('_', "-"))
}

/// What each category is for, beyond the matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CategoryFlags {
    /// A new asset may be created in it.
    pub start: bool,
    /// A loan may be issued from it.
    pub loanable: bool,
    /// A rule's assign step hands out an asset from it.
    pub assignable: bool,
    /// A replaced asset may be moved to it by the replacement's carry-over.
    pub old_asset_target: bool,
    /// Nothing moves an asset out of it.
    pub terminal: bool,
}

/// The flags of `category`.
pub const fn flags(category: AssetStatus) -> CategoryFlags {
    use AssetStatus::*;
    let (start, loanable, assignable, old_asset_target, terminal) = match category {
        OnOrder => (true, false, true, false, false),
        InTransit => (true, false, true, false, false),
        InStock => (true, true, true, true, false),
        InService => (true, true, true, false, false),
        OnLoan => (false, false, false, false, false),
        InRepair => (true, false, false, false, false),
        Lost => (true, false, false, false, false),
        Retired => (true, false, false, true, false),
        Disposed => (false, false, false, true, true),
    };
    CategoryFlags {
        start,
        loanable,
        assignable,
        old_asset_target,
        terminal,
    }
}

/// What the matrix says of a move between two different categories, before
/// the facts of the asset are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    Allowed,
    /// Allowed with the disposal record.
    WithDisposal,
    /// By issuing a loan; by hand only while the asset has an active loan,
    /// to repair a status that drifted off `on_loan`.
    WithActiveLoan,
    Refused(RefusalCode),
}

/// A move matrix: the cell for `from` to `to`, two different categories.
pub type Matrix = fn(AssetStatus, AssetStatus) -> Cell;

/// A move back into `on_order` after the asset was created (decision
/// B-D3): an asset starts on order, set at create (REST or import), and
/// nothing returns it there. `Cell::Allowed` allows the moves again.
pub const INTO_ON_ORDER: Cell = Cell::Refused(RefusalCode::StartOnly);

/// The fixed matrix every path uses in 1.2.
pub const FIXED: Matrix = fixed_cell;

fn fixed_cell(from: AssetStatus, to: AssetStatus) -> Cell {
    use AssetStatus::*;
    match (from, to) {
        (Disposed, _) => Cell::Refused(RefusalCode::Terminal),
        (_, OnOrder) => INTO_ON_ORDER,
        (_, OnLoan) => Cell::WithActiveLoan,
        (_, Disposed) => Cell::WithDisposal,
        _ => Cell::Allowed,
    }
}

/// Who moves the asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mover {
    /// A person, a rule step, an import or a replacement's carry-over.
    Hand,
    /// The loan ledger issuing a loan: into `on_loan`.
    LoanIssue,
    /// The loan ledger returning a loan: off `on_loan`, to the category the
    /// loan recorded.
    LoanReturn,
}

/// What the caller knows of the asset and the move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveFacts {
    pub mover: Mover,
    /// The asset has a loan not yet returned.
    pub active_loan: bool,
    /// The move carries a disposal record.
    pub disposal: bool,
}

impl MoveFacts {
    /// A move by hand.
    pub fn by_hand(active_loan: bool, disposal: bool) -> Self {
        Self {
            mover: Mover::Hand,
            active_loan,
            disposal,
        }
    }
}

/// Whether a new asset may start in `category`.
pub fn check_initial(category: AssetStatus) -> Result<(), MoveRefused> {
    if flags(category).start {
        return Ok(());
    }
    let code = match category {
        AssetStatus::Disposed => RefusalCode::NeedsDisposal,
        _ => RefusalCode::StartOnLoan,
    };
    Err(MoveRefused {
        code,
        from: None,
        to: category,
    })
}

/// Whether an asset in `from` may move to `to`, by the fixed matrix.
pub fn check_move(from: AssetStatus, to: AssetStatus, facts: MoveFacts) -> Result<(), MoveRefused> {
    check_move_in(FIXED, from, to, facts)
}

/// [`check_move`] by `matrix`. The order is fixed: same category, the loan
/// ledger's own moves, a loan that owns the asset, then the cell.
pub fn check_move_in(
    matrix: Matrix,
    from: AssetStatus,
    to: AssetStatus,
    facts: MoveFacts,
) -> Result<(), MoveRefused> {
    let refuse = |code| {
        Err(MoveRefused {
            code,
            from: Some(from),
            to,
        })
    };
    match facts.mover {
        // The ledger puts back the category the loan recorded, from on loan
        // only; an asset that drifted to that very category isn't on loan.
        Mover::LoanReturn if from != AssetStatus::OnLoan => return refuse(RefusalCode::NotOnLoan),
        Mover::LoanReturn if to == AssetStatus::OnLoan => return refuse(RefusalCode::SameStatus),
        Mover::LoanReturn => return Ok(()),
        Mover::LoanIssue if from == AssetStatus::OnLoan => return refuse(RefusalCode::SameStatus),
        Mover::LoanIssue if !flags(from).loanable => return refuse(RefusalCode::NotLoanable),
        Mover::LoanIssue => return Ok(()),
        Mover::Hand => {}
    }
    if from == to {
        return refuse(RefusalCode::SameStatus);
    }
    // A loan out owns the asset: off on loan only by the return, and never
    // into disposed, whatever status it drifted to, so no disposed asset is
    // left holding a loan.
    if facts.active_loan && (from == AssetStatus::OnLoan || to == AssetStatus::Disposed) {
        return refuse(RefusalCode::LoanOwned);
    }
    match matrix(from, to) {
        Cell::Refused(code) => refuse(code),
        Cell::WithDisposal if facts.disposal => Ok(()),
        Cell::WithDisposal => refuse(RefusalCode::NeedsDisposal),
        _ if facts.disposal => refuse(RefusalCode::DisposalNotAllowed),
        Cell::WithActiveLoan if facts.active_loan => Ok(()),
        Cell::WithActiveLoan => refuse(RefusalCode::NeedsActiveLoan),
        Cell::Allowed => Ok(()),
    }
}

/// What a manual move needs beyond the matrix, for a client to offer it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Need {
    /// The disposal record.
    Disposal,
    /// A loan on the asset not yet returned.
    ActiveLoan,
    /// No such loan.
    NoActiveLoan,
}

/// A category a manual move from another may go to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Target {
    pub key: AssetStatus,
    pub needs: Vec<Need>,
}

/// The categories a move by hand may take an asset in `from` to, each with
/// what it needs. Read off [`check_move`] for every combination of facts, so
/// it can't disagree with the write.
pub fn manual_targets(from: AssetStatus) -> Vec<Target> {
    let mut out = Vec::new();
    for to in AssetStatus::ALL {
        let allowed: Vec<MoveFacts> = [false, true]
            .into_iter()
            .flat_map(|loan| [false, true].map(|disposal| MoveFacts::by_hand(loan, disposal)))
            .filter(|facts| check_move(from, to, *facts).is_ok())
            .collect();
        if allowed.is_empty() {
            continue;
        }
        let mut needs = Vec::new();
        if allowed.iter().all(|f| f.disposal) {
            needs.push(Need::Disposal);
        }
        if allowed.iter().all(|f| f.active_loan) {
            needs.push(Need::ActiveLoan);
        } else if allowed.iter().all(|f| !f.active_loan) {
            needs.push(Need::NoActiveLoan);
        }
        out.push(Target { key: to, needs });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use AssetStatus::*;

    /// What a manual move by hand comes to, written out per cell.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Expect {
        Ok,
        /// Allowed with the disposal record.
        Disposal,
        /// Allowed while the asset has an active loan.
        Loan,
        /// Allowed only with no active loan (a stranded `on_loan`).
        NoLoan,
        /// Both: no active loan and the disposal record.
        DisposalNoLoan,
        No(RefusalCode),
    }

    /// The design's matrix, cell by cell (rows: from; columns: to, in
    /// `AssetStatus::ALL` order). The on_order column reads the named cell.
    fn expected(from: AssetStatus, to: AssetStatus) -> Expect {
        use Expect::*;
        use RefusalCode as R;
        let into_on_order = match INTO_ON_ORDER {
            Cell::Allowed => Ok,
            Cell::Refused(code) => No(code),
            other => panic!("INTO_ON_ORDER is {other:?}; only Allowed or Refused make sense"),
        };
        if from == to {
            return No(R::SameStatus);
        }
        match from {
            Disposed => No(R::Terminal),
            // Stranded on loan: the matrix; with a loan out, the loan owns it.
            OnLoan => match to {
                OnOrder if into_on_order == Ok => NoLoan,
                OnOrder => into_on_order,
                Disposed => DisposalNoLoan,
                _ => NoLoan,
            },
            _ => match to {
                OnOrder => into_on_order,
                OnLoan => Loan,
                // A loan out keeps it from being disposed.
                Disposed => DisposalNoLoan,
                _ => Ok,
            },
        }
    }

    fn actual(from: AssetStatus, to: AssetStatus) -> Expect {
        let ok = |loan, disposal| check_move(from, to, MoveFacts::by_hand(loan, disposal)).is_ok();
        match (
            ok(false, false),
            ok(true, false),
            ok(false, true),
            ok(true, true),
        ) {
            (true, true, false, false) => Expect::Ok,
            (false, false, true, true) => Expect::Disposal,
            (false, true, false, false) => Expect::Loan,
            (true, false, false, false) => Expect::NoLoan,
            (false, false, true, false) => Expect::DisposalNoLoan,
            (false, false, false, false) => Expect::No(
                check_move(from, to, MoveFacts::by_hand(false, false))
                    .unwrap_err()
                    .code,
            ),
            other => panic!("{from:?} -> {to:?}: unexpected shape {other:?}"),
        }
    }

    #[test]
    fn every_cell_of_the_matrix() {
        for from in AssetStatus::ALL {
            for to in AssetStatus::ALL {
                assert_eq!(actual(from, to), expected(from, to), "{from:?} -> {to:?}");
            }
        }
    }

    #[test]
    fn a_loan_owns_the_asset_until_it_is_returned() {
        for to in AssetStatus::ALL.into_iter().filter(|s| *s != OnLoan) {
            for disposal in [false, true] {
                let err = check_move(OnLoan, to, MoveFacts::by_hand(true, disposal)).unwrap_err();
                assert_eq!(err.code, RefusalCode::LoanOwned, "{to:?}");
            }
        }
    }

    #[test]
    fn refusals_come_in_a_fixed_order() {
        let code = |from, to, loan, disposal| {
            check_move(from, to, MoveFacts::by_hand(loan, disposal))
                .unwrap_err()
                .code
        };
        // Same beats everything, so a bulk caller reads it as unchanged.
        assert_eq!(
            code(Disposed, Disposed, false, false),
            RefusalCode::SameStatus
        );
        assert_eq!(code(OnLoan, OnLoan, true, true), RefusalCode::SameStatus);
        // Terminal beats the disposal record's shape.
        assert_eq!(code(Disposed, Retired, false, true), RefusalCode::Terminal);
        // A loan out beats the record's shape and the cell.
        assert_eq!(code(OnLoan, Disposed, true, false), RefusalCode::LoanOwned);
        // A loan out keeps an asset from disposed, whatever its status.
        assert_eq!(code(InStock, Disposed, true, true), RefusalCode::LoanOwned);
        assert_eq!(code(InRepair, Disposed, true, true), RefusalCode::LoanOwned);
        // A record sent with another target is refused before the loan rule.
        assert_eq!(
            code(InStock, OnLoan, true, true),
            RefusalCode::DisposalNotAllowed
        );
        assert_eq!(
            code(InStock, Retired, false, true),
            RefusalCode::DisposalNotAllowed
        );
    }

    #[test]
    fn the_ledger_issues_from_loanable_categories_and_returns_from_on_loan() {
        let issue = |from| {
            check_move(
                from,
                OnLoan,
                MoveFacts {
                    mover: Mover::LoanIssue,
                    active_loan: false,
                    disposal: false,
                },
            )
        };
        for from in AssetStatus::ALL.into_iter().filter(|s| *s != OnLoan) {
            let got = issue(from).map_err(|e| e.code);
            if flags(from).loanable {
                assert_eq!(got, Ok(()), "{from:?}");
            } else {
                assert_eq!(got, Err(RefusalCode::NotLoanable), "{from:?}");
            }
        }
        let back = |from, to| {
            check_move(
                from,
                to,
                MoveFacts {
                    mover: Mover::LoanReturn,
                    active_loan: true,
                    disposal: false,
                },
            )
            .map_err(|e| e.code)
        };
        assert_eq!(back(OnLoan, InStock), Ok(()));
        assert_eq!(back(OnLoan, InService), Ok(()));
        assert_eq!(back(InService, InStock), Err(RefusalCode::NotOnLoan));
        // Drifted to the very status the loan recorded: still not on loan.
        assert_eq!(back(InStock, InStock), Err(RefusalCode::NotOnLoan));
        assert_eq!(back(Disposed, InStock), Err(RefusalCode::NotOnLoan));
    }

    #[test]
    fn a_new_asset_starts_in_any_category_but_on_loan_and_disposed() {
        for status in AssetStatus::ALL {
            let got = check_initial(status).map_err(|e| e.code);
            let want = match status {
                OnLoan => Err(RefusalCode::StartOnLoan),
                Disposed => Err(RefusalCode::NeedsDisposal),
                _ => Ok(()),
            };
            assert_eq!(got, want, "{status:?}");
            assert_eq!(flags(status).start, want.is_ok(), "{status:?}");
        }
        assert!(check_initial(OnOrder).is_ok());
    }

    #[test]
    fn the_flags_are_the_lists_they_replace() {
        let with = |f: fn(CategoryFlags) -> bool| -> Vec<&str> {
            AssetStatus::ALL
                .into_iter()
                .filter(|s| f(flags(*s)))
                .map(AssetStatus::as_str)
                .collect()
        };
        assert_eq!(with(|f| f.loanable), ["in_service", "in_stock"]);
        assert_eq!(
            with(|f| f.assignable),
            ["in_service", "in_stock", "on_order", "in_transit"]
        );
        assert_eq!(
            with(|f| f.old_asset_target),
            ["in_stock", "retired", "disposed"]
        );
        assert_eq!(with(|f| f.terminal), ["disposed"]);
    }

    #[test]
    fn targets_say_what_each_move_needs() {
        let of = |from| {
            manual_targets(from)
                .into_iter()
                .map(|t| (t.key, t.needs))
                .collect::<Vec<_>>()
        };
        let in_service = of(InService);
        assert!(in_service.contains(&(InStock, vec![])));
        assert!(in_service.contains(&(OnLoan, vec![Need::ActiveLoan])));
        assert!(in_service.contains(&(Disposed, vec![Need::Disposal, Need::NoActiveLoan])));
        assert!(!in_service.iter().any(|(k, _)| *k == InService));
        assert_eq!(
            in_service.iter().any(|(k, _)| *k == OnOrder),
            INTO_ON_ORDER == Cell::Allowed
        );
        // Off on loan only with no loan out.
        for (key, needs) in of(OnLoan) {
            assert!(needs.contains(&Need::NoActiveLoan), "{key:?}");
        }
        assert!(of(Disposed).is_empty());
    }

    #[test]
    fn every_refusal_has_its_words() {
        let codes = [
            RefusalCode::SameStatus,
            RefusalCode::Terminal,
            RefusalCode::LoanOwned,
            RefusalCode::NeedsActiveLoan,
            RefusalCode::NotLoanable,
            RefusalCode::NotOnLoan,
            RefusalCode::NeedsDisposal,
            RefusalCode::DisposalNotAllowed,
            RefusalCode::StartOnly,
            RefusalCode::StartOnLoan,
        ];
        for code in codes {
            let message = MoveRefused {
                code,
                from: Some(InRepair),
                to: OnOrder,
            }
            .message();
            assert!(!message.starts_with('{'), "{code:?}: {message}");
            assert!(!message.contains("{ $"), "{code:?}: {message}");
        }
        let start_only = MoveRefused {
            code: RefusalCode::StartOnly,
            from: Some(InStock),
            to: OnOrder,
        };
        assert!(
            start_only.message().contains("On order"),
            "{}",
            start_only.message()
        );
    }
}
