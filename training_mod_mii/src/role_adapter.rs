//! Owned dispatch for the native Training saved-original same-class roles.
//! No native calls, pointer dereferences, synchronization, file writes or UI registration.
//! Copies below do NOT prove native readiness or pin an entry/pool. The native caller must
//! establish current callback/scene/INSTALL authority and rederive them under G before writing.

use crate::inventory::CHARINFO_LEN;
use crate::lifetime::{
    self, Configuration, MiiOverride, Observed, Operation, Phase, Ticket, Transaction,
};
use crate::native_values::{
    self as values, CpuHelper, CpuOwner, Mirrors, RecordUpdate, RowIds, RECORD_LEN,
};
use crate::selection::{Action, Choice, Role};

/// Fixed-size owned scope, including inactive assigned records. Exactly one branch must exist.
/// No spare-record inference or heap allocation.
/// The CPU form intentionally retains ordinary Luigi P1 and native helpers2/3; it does not
/// authorize two simultaneously visible Miis, LDN owner4, or any ordinary-origin transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub player_kinds: Option<[Option<u32>; 8]>,
    pub cpu: Option<CpuScope>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuScope {
    pub owners: [CpuOwner; 4],
    pub helpers: [CpuHelper; 2],
    pub other_entries_present: bool,
}

impl Scope {
    pub fn player(owner_kinds: [Option<u32>; 8]) -> Self {
        Self {
            player_kinds: Some(owner_kinds),
            cpu: None,
        }
    }
    pub fn cpu(
        owners: [CpuOwner; 4],
        helpers: [CpuHelper; 2],
        other_entries_present: bool,
    ) -> Self {
        Self {
            player_kinds: None,
            cpu: Some(CpuScope {
                owners,
                helpers,
                other_entries_present,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Current {
    pub live: Observed,
    pub thread: usize,
    pub information: usize,
    pub object: usize,
    pub scope: Scope,
    pub mirrors: Mirrors,
    pub row_ids: RowIds,
    pub record: [u8; RECORD_LEN],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Scope,
    ContextChanged,
    WrongRole,
    Lifetime(lifetime::Error),
    Values(values::Error),
}
impl From<lifetime::Error> for Error {
    fn from(value: lifetime::Error) -> Self {
        Self::Lifetime(value)
    }
}
impl From<values::Error> for Error {
    fn from(value: values::Error) -> Self {
        Self::Values(value)
    }
}

impl Current {
    fn validate(&self, role: Role) -> Result<(), Error> {
        let b = self.live.binding;
        let aligned = |p: usize| p != 0 && p.is_multiple_of(8);
        if b.epoch == 0
            || self.thread == 0
            || !aligned(self.information)
            || !aligned(self.object)
        {
            return Err(Error::Scope);
        }
        values::record_address(b, b)?;
        self.mirrors
            .validate(b.record_index, self.live.configuration.tokens())?;
        self.row_ids.validate(self.live.configuration)?;
        if self.record[..CHARINFO_LEN] != *self.live.configuration.charinfo() {
            return Err(values::Error::RecordChanged.into());
        }
        match (role, self.scope.player_kinds, self.scope.cpu) {
            (Role::Player, Some(owner_kinds), None) if b.record_index == 0 => {
                values::qualified_saved_scope(b, self.live.configuration.canonical().class as u32, owner_kinds)?;
            }
            (
                Role::Cpu,
                None,
                Some(CpuScope {
                    owners,
                    helpers,
                    other_entries_present,
                }),
            ) => {
                values::qualified_saved_cpu_scope(&owners, other_entries_present, self.live.configuration.canonical().class as u32)?;
                let target = owners[1];
                if b.owner != target.owner
                    || b.record_index != target.index
                    || b.entry != target.entry
                    || b.row != target.row
                    || self.information != target.information
                    || self.object != target.object
                {
                    return Err(Error::Scope);
                }
                for (helper, owner) in helpers.iter().zip(&owners[2..]) {
                    if helper.owner != *owner || !aligned(helper.body) {
                        return Err(Error::Scope);
                    }
                    helper
                        .mirrors
                        .validate(owner.index, helper.mirrors.tokens)?;
                }
            }
            _ => return Err(Error::Scope),
        }
        Ok(())
    }

    fn same_authority(&self, previous: &Self) -> Result<(), Error> {
        if self.live.binding != previous.live.binding
            || self.thread != previous.thread
            || self.information != previous.information
            || self.object != previous.object
            || self.scope != previous.scope
        {
            return Err(Error::ContextChanged);
        }
        Ok(())
    }
}

/// One non-cloneable value per role/owner lifetime. No global queue or shared original snapshot.
/// Before ANY scene roster install, every live alternate must restore/complete or that install
/// must be blocked: unrelated owner changes invalidate the frozen scope too. Never relax that
/// equality to rescue a restore after a caller has already violated this ordering.
/// A caller may keep separate Player/CPU slots, but current native scope still rejects the
/// unqualified simultaneous-Mii layout. This object is not a native resource lease.
pub struct SavedRole {
    role: Role,
    original: Current,
    state: MiiOverride,
}

impl SavedRole {
    /// The caller has freshly established native Training, actual current role, full readiness,
    /// no pending native mutation, fingerprints, and the complete copied scope under G.
    pub fn capture(role: Role, original: Current) -> Result<Self, Error> {
        original.validate(role)?;
        if let Some(CpuScope { helpers, .. }) = original.scope.cpu {
            // Native helpers are copies of the ORIGINAL saved configuration, never of an
            // alternate requested later. Compare full bytes, not masked catalog identity.
            for helper in helpers {
                if helper.record[..CHARINFO_LEN] != *original.live.configuration.charinfo() {
                    return Err(values::Error::RecordChanged.into());
                }
                helper
                    .mirrors
                    .validate(helper.owner.index, original.live.configuration.tokens())?;
            }
        }
        let state = MiiOverride::capture(role, original.live)?;
        Ok(Self {
            role,
            original,
            state,
        })
    }

    fn current(&self, current: &Current) -> Result<(), Error> {
        current.validate(self.role)?;
        current.same_authority(&self.original)
    }

    pub fn phase(&self) -> Phase {
        self.state.phase()
    }
    pub fn original(&self) -> Configuration {
        self.state.original()
    }
    pub fn expected(&self) -> Configuration {
        self.state.expected()
    }
    pub fn pending_ticket(&self) -> Option<Ticket> {
        self.state.pending_ticket()
    }

    /// Read-only menu recapture: never rebase the original, expected bytes or scope.
    pub fn menu_current(&self, current: Current) -> Result<(), Error> {
        self.current(&current)?;
        if self.phase() != Phase::Ready || current.live.configuration != self.expected() {
            return Err(Error::ContextChanged);
        }
        Ok(())
    }

    /// Consume the matching role's existing menu-close value; file/catalog resolution happens
    /// outside this module. Different role/class/hat and P1's protected slot0 remain refused.
    pub fn select(
        &mut self,
        request_epoch: u64,
        current: Current,
        action: Action,
        target: Configuration,
    ) -> Result<Option<Transaction>, Error> {
        if request_epoch != self.original.live.binding.epoch {
            return Err(Error::ContextChanged);
        }
        let choice: Choice = match (self.role, action) {
            (Role::Player, Action::ReloadPlayer(choice))
            | (Role::Cpu, Action::ReloadCpu(choice)) => choice,
            _ => return Err(Error::WrongRole),
        };
        self.current(&current)?;
        if choice.canonical != target.canonical() {
            return Err(lifetime::Error::ChoiceMismatch.into());
        }
        // CPU may select its original. Restore exact captured bytes, never another dedup copy.
        if self.role == Role::Cpu && target.canonical() == self.original().canonical() {
            return Ok(self.state.restore(current.live)?);
        }
        Ok(self.state.apply(current.live, choice, target)?)
    }

    pub fn restore(&mut self, current: Current) -> Result<Option<Transaction>, Error> {
        self.current(&current)?;
        Ok(self.state.restore(current.live)?)
    }

    pub fn can_change_kind_or_return_slot0(&self, current: Current) -> Result<(), Error> {
        self.current(&current)?;
        Ok(self.state.can_change_kind_or_return_slot0(current.live)?)
    }

    /// Only after obtaining the separate native InstallToken. Do this outside G and release
    /// Rust state locks before native calls. The returned owned commit has no native authority.
    pub fn submit(&mut self, ticket: Ticket, current: Current) -> Result<Commit, Error> {
        self.current(&current)?;
        let transaction = self.state.submit(ticket, current.live)?;
        Ok(Commit {
            role: self.role,
            original: self.original,
            transaction,
        })
    }

    /// Full native readiness and a freshly rederived target/record are prerequisites. A failed
    /// copied-value check retains the pending ticket and refuses future writes.
    pub fn complete(&mut self, ticket: Ticket, current: Current) -> Result<Operation, Error> {
        if let Err(error) = self.current(&current) {
            self.state.refuse(ticket)?;
            return Err(error);
        }
        Ok(self.state.complete(ticket, current.live)?)
    }
    /// Only the native adapter's proven pre-write/pre-INSTALL refusal permits recovery.
    pub fn abandon_unwritten(&mut self, ticket: Ticket) -> Result<(), Error> {
        Ok(self.state.abandon_unwritten(ticket)?)
    }
    pub fn cancel_prepared(&mut self, ticket: Ticket) -> Result<(), Error> {
        Ok(self.state.cancel_prepared(ticket)?)
    }
    pub fn refuse(&mut self, ticket: Ticket) -> Result<(), Error> {
        Ok(self.state.refuse(ticket)?)
    }
    /// No write or token release; current native scene/owner destruction remains authoritative.
    pub fn depart(&mut self) {
        self.state.depart();
    }
    pub fn settle_terminal(&mut self, ticket: Ticket) -> Result<(), Error> {
        Ok(self.state.settle_terminal(ticket)?)
    }
}

/// Owned handoff across the Rust-lock/native-lock boundary, not Clone and not a native token.
pub struct Commit {
    role: Role,
    original: Current,
    transaction: Transaction,
}
impl Commit {
    pub fn transaction(&self) -> Transaction {
        self.transaction
    }

    /// Invoke only on fresh copies rederived inside qualified G, on the current callback/thread,
    /// while the actual InstallToken still matches. Getters supplying target_rows ran before G.
    /// All checks are pure; the returned payload changes only a copied record's first88bytes.
    pub fn prepare_write(
        self,
        current: Current,
        target_rows: RowIds,
    ) -> Result<RecordUpdate, Error> {
        current.validate(self.role)?;
        current.same_authority(&self.original)?;
        if current.live.configuration != self.transaction.before() {
            return Err(lifetime::Error::UnexpectedConfiguration.into());
        }
        Ok(RecordUpdate::prepare_for_role(
            self.role,
            self.transaction,
            current.live.binding,
            current.mirrors,
            [current.row_ids, target_rows],
            &current.record,
        )?)
    }
}
