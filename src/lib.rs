//! Criome domain runtime library.
//!
//! The daemon owns the in-memory registry and policy store. The CLI is only a
//! text-to-Signal adapter for this daemon.

use std::net::IpAddr;
use std::path::Path;
use std::sync::Mutex;

use nota_codec::NotaRecord;
use owner_signal_domain_criome::{
    Delegation as OwnerDelegation, DomainDelegated, DomainRegistered, DomainRetired,
    Operation as OwnerOperation, Policy, PolicySet, ProjectionDirective, ProjectionPolicy,
    Registration, Reply as OwnerReply, RequestRejected as OwnerRequestRejected, Retirement,
};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_domain_criome::{
    Address, Delegation as DomainDelegation, DelegationListing, DelegationQuery, DomainListing,
    DomainName, DomainNameSystemRecord, DomainQuery, NetworkAddress, Observation,
    ObservationResult, Operation as DomainOperation, Projection, ProjectionQuery, ProjectionScope,
    RecordKind, RecordValue, Reply as DomainReply, RequestRejected, ResolutionQuery,
    ResolutionResult,
};
use signal_frame::{NonEmpty, Reply as FrameReply, SubReply};

pub mod client;
pub mod daemon;
pub mod frame_io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("signal frame error: {0}")]
    Frame(#[from] signal_frame::FrameError),

    #[error("command-line route error: {0}")]
    CommandLineRoute(#[from] signal_frame::CommandLineRouteError),

    #[error("NOTA decode error: {0}")]
    Nota(#[from] nota_codec::Error),

    #[error("configuration decode error: {0}")]
    Configuration(#[from] nota_config::Error),

    #[error("expected exactly one argument")]
    ExpectedSingleArgument,

    #[error("flag-style arguments are not part of component binaries: {0}")]
    FlagArgument(String),

    #[error("unexpected signal frame for this socket")]
    UnexpectedFrame,

    #[error("trailing input after single NOTA request")]
    TrailingInput,

    #[error("connection closed before a complete frame arrived")]
    ConnectionClosed,

    #[error("signal handshake was rejected")]
    HandshakeRejected,

    #[error("signal request was rejected before execution")]
    SignalRequestRejected,

    #[error("signal request failed during execution")]
    SignalRequestFailed,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn command_line_route(error: signal_frame::CommandLineRouteError) -> Self {
        Self::CommandLineRoute(error)
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfiguration {
    pub ordinary_socket_path: String,
    pub ordinary_socket_mode: u32,
    pub owner_socket_path: String,
    pub owner_socket_mode: u32,
}

nota_config::impl_rkyv_configuration!(DaemonConfiguration);

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegisteredDelegation {
    name: signal_domain_criome::DelegationName,
    domain: DomainName,
    target: owner_signal_domain_criome::DelegationTarget,
}

impl RegisteredDelegation {
    fn from_owner(delegation: OwnerDelegation) -> Self {
        Self {
            name: delegation.name,
            domain: delegation.domain,
            target: delegation.target,
        }
    }

    fn as_listing_entry(&self) -> DomainDelegation {
        DomainDelegation {
            name: self.name.clone(),
            domain: self.domain.clone(),
        }
    }

    fn fully_qualified_name(&self) -> DomainName {
        if self.name.as_str() == "@" {
            return self.domain.clone();
        }
        DomainName::new(format!("{}.{}", self.name.as_str(), self.domain.as_str()))
    }

    fn as_record(&self) -> DomainNameSystemRecord {
        let record_kind = match self.target.as_str().parse::<IpAddr>() {
            Ok(IpAddr::V4(_)) => RecordKind::AddressV4,
            Ok(IpAddr::V6(_)) => RecordKind::AddressV6,
            Err(_) => RecordKind::CanonicalName,
        };
        DomainNameSystemRecord {
            name: self.fully_qualified_name(),
            kind: record_kind,
            value: RecordValue::new(self.target.as_str()),
        }
    }

    fn address(&self) -> Option<Address> {
        self.target.as_str().parse::<IpAddr>().ok()?;
        Some(Address {
            name: self.fully_qualified_name(),
            address: NetworkAddress::new(self.target.as_str()),
        })
    }
}

#[derive(Debug)]
pub struct Store {
    domains: Mutex<Vec<DomainName>>,
    delegations: Mutex<Vec<RegisteredDelegation>>,
    policy: Mutex<Policy>,
}

impl Store {
    pub fn new() -> Self {
        Self {
            domains: Mutex::new(Vec::new()),
            delegations: Mutex::new(Vec::new()),
            policy: Mutex::new(Policy {
                projections: Vec::new(),
            }),
        }
    }

    pub fn open(_path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::new())
    }

    pub fn handle_ordinary_request(
        &self,
        request: signal_domain_criome::ChannelRequest,
    ) -> signal_domain_criome::ChannelReply {
        let replies = request
            .payloads
            .into_iter()
            .map(|operation| SubReply::Ok(self.handle_ordinary_operation(operation)))
            .collect::<Vec<_>>();
        FrameReply::committed(
            NonEmpty::try_from_vec(replies).expect("signal request is guaranteed non-empty"),
        )
    }

    pub fn handle_owner_request(
        &self,
        request: owner_signal_domain_criome::ChannelRequest,
    ) -> owner_signal_domain_criome::ChannelReply {
        let replies = request
            .payloads
            .into_iter()
            .map(|operation| SubReply::Ok(self.handle_owner_operation(operation)))
            .collect::<Vec<_>>();
        FrameReply::committed(
            NonEmpty::try_from_vec(replies).expect("signal request is guaranteed non-empty"),
        )
    }

    fn handle_ordinary_operation(&self, operation: DomainOperation) -> DomainReply {
        match operation {
            DomainOperation::Observe(observation) => self.observe(observation),
            DomainOperation::Resolve(query) => self.resolve(query),
            DomainOperation::Project(query) => self.project(query),
        }
    }

    fn observe(&self, observation: Observation) -> DomainReply {
        match observation {
            Observation::Domains(query) => {
                DomainReply::Observed(ObservationResult::Domains(self.domains(query)))
            }
            Observation::Delegations(query) => {
                DomainReply::Observed(ObservationResult::Delegations(self.delegations(query)))
            }
            Observation::Projection(query) => match self
                .projection_for(query, signal_domain_criome::OperationKind::Observe)
            {
                Ok(projection) => DomainReply::Observed(ObservationResult::Projection(projection)),
                Err(rejected) => DomainReply::RequestRejected(rejected),
            },
        }
    }

    fn domains(&self, query: DomainQuery) -> DomainListing {
        let mut domains = self
            .domains
            .lock()
            .expect("domain registry mutex should not be poisoned")
            .iter()
            .filter(|domain| match &query.root {
                Some(root) => domain_is_under_root(domain, root),
                None => true,
            })
            .cloned()
            .collect::<Vec<_>>();
        domains.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        DomainListing { domains }
    }

    fn delegations(&self, query: DelegationQuery) -> DelegationListing {
        let mut delegations = self
            .delegations
            .lock()
            .expect("delegation registry mutex should not be poisoned")
            .iter()
            .filter(|delegation| {
                query
                    .domain
                    .as_ref()
                    .is_none_or(|domain| delegation.domain == *domain)
            })
            .map(RegisteredDelegation::as_listing_entry)
            .collect::<Vec<_>>();
        delegations.sort_by(|left, right| {
            left.domain
                .as_str()
                .cmp(right.domain.as_str())
                .then_with(|| left.name.as_str().cmp(right.name.as_str()))
        });
        DelegationListing { delegations }
    }

    fn resolve(&self, query: ResolutionQuery) -> DomainReply {
        if self.domain_exists(&query.name) {
            return DomainReply::Resolved(ResolutionResult {
                query,
                addresses: Vec::new(),
            });
        }
        if let Some(delegation) = self.delegation_for_name(&query.name) {
            return DomainReply::Resolved(ResolutionResult {
                query,
                addresses: delegation.address().into_iter().collect(),
            });
        }
        DomainReply::RequestRejected(RequestRejected {
            operation: signal_domain_criome::OperationKind::Resolve,
            reason: signal_domain_criome::RejectionReason::DomainUnknown,
        })
    }

    fn project(&self, query: ProjectionQuery) -> DomainReply {
        match self.projection_for(query, signal_domain_criome::OperationKind::Project) {
            Ok(projection) => DomainReply::Projected(projection),
            Err(rejected) => DomainReply::RequestRejected(rejected),
        }
    }

    fn projection_for(
        &self,
        query: ProjectionQuery,
        operation: signal_domain_criome::OperationKind,
    ) -> std::result::Result<Projection, RequestRejected> {
        if !self.domain_exists(&query.domain) {
            return Err(RequestRejected {
                operation,
                reason: signal_domain_criome::RejectionReason::DomainUnknown,
            });
        }
        if !self.projection_enabled(&query.domain, query.scope) {
            return Err(RequestRejected {
                operation,
                reason: signal_domain_criome::RejectionReason::ProjectionUnavailable,
            });
        }
        if includes_redirect_rules(query.scope) {
            return Err(RequestRejected {
                operation,
                reason: signal_domain_criome::RejectionReason::ProjectionUnavailable,
            });
        }

        let records = if includes_public_records(query.scope) {
            self.records_for_domain(&query.domain)
        } else {
            Vec::new()
        };
        Ok(Projection {
            query,
            records,
            redirects: Vec::new(),
        })
    }

    fn records_for_domain(&self, domain: &DomainName) -> Vec<DomainNameSystemRecord> {
        let mut records = self
            .delegations
            .lock()
            .expect("delegation registry mutex should not be poisoned")
            .iter()
            .filter(|delegation| delegation.domain == *domain)
            .map(RegisteredDelegation::as_record)
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            left.name
                .as_str()
                .cmp(right.name.as_str())
                .then_with(|| format!("{:?}", left.kind).cmp(&format!("{:?}", right.kind)))
                .then_with(|| left.value.as_str().cmp(right.value.as_str()))
        });
        records
    }

    fn handle_owner_operation(&self, operation: OwnerOperation) -> OwnerReply {
        match operation {
            OwnerOperation::RegisterDomain(registration) => self.register_domain(registration),
            OwnerOperation::Delegate(delegation) => self.delegate(delegation),
            OwnerOperation::RetireDomain(retirement) => self.retire_domain(retirement),
            OwnerOperation::SetPolicy(policy) => self.set_policy(policy),
        }
    }

    fn register_domain(&self, registration: Registration) -> OwnerReply {
        let mut domains = self
            .domains
            .lock()
            .expect("domain registry mutex should not be poisoned");
        if domains.iter().any(|domain| domain == &registration.domain) {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                operation: owner_signal_domain_criome::OperationKind::RegisterDomain,
                reason: owner_signal_domain_criome::RejectionReason::DomainAlreadyRegistered,
            });
        }
        domains.push(registration.domain.clone());
        OwnerReply::DomainRegistered(DomainRegistered {
            domain: registration.domain,
        })
    }

    fn delegate(&self, delegation: OwnerDelegation) -> OwnerReply {
        if !self.domain_exists(&delegation.domain) {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                operation: owner_signal_domain_criome::OperationKind::Delegate,
                reason: owner_signal_domain_criome::RejectionReason::DomainUnknown,
            });
        }

        let mut delegations = self
            .delegations
            .lock()
            .expect("delegation registry mutex should not be poisoned");
        if delegations.iter().any(|existing| {
            existing.domain == delegation.domain && existing.name == delegation.name
        }) {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                operation: owner_signal_domain_criome::OperationKind::Delegate,
                reason: owner_signal_domain_criome::RejectionReason::DelegationAlreadyExists,
            });
        }
        let reply = OwnerReply::DomainDelegated(DomainDelegated {
            name: delegation.name.clone(),
            domain: delegation.domain.clone(),
        });
        delegations.push(RegisteredDelegation::from_owner(delegation));
        reply
    }

    fn retire_domain(&self, retirement: Retirement) -> OwnerReply {
        if !self.domain_exists(&retirement.domain) {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                operation: owner_signal_domain_criome::OperationKind::RetireDomain,
                reason: owner_signal_domain_criome::RejectionReason::DomainUnknown,
            });
        }
        self.domains
            .lock()
            .expect("domain registry mutex should not be poisoned")
            .retain(|domain| domain != &retirement.domain);
        self.delegations
            .lock()
            .expect("delegation registry mutex should not be poisoned")
            .retain(|delegation| delegation.domain != retirement.domain);
        self.policy
            .lock()
            .expect("policy mutex should not be poisoned")
            .projections
            .retain(|policy| policy.domain != retirement.domain);
        OwnerReply::DomainRetired(DomainRetired {
            domain: retirement.domain,
        })
    }

    fn set_policy(&self, policy: Policy) -> OwnerReply {
        if policy
            .projections
            .iter()
            .any(|projection| !self.domain_exists(&projection.domain))
        {
            return OwnerReply::RequestRejected(OwnerRequestRejected {
                operation: owner_signal_domain_criome::OperationKind::SetPolicy,
                reason: owner_signal_domain_criome::RejectionReason::DomainUnknown,
            });
        }

        let projection_policy_count = policy.projections.len() as u64;
        *self
            .policy
            .lock()
            .expect("policy mutex should not be poisoned") = policy;
        OwnerReply::PolicySet(PolicySet {
            projection_policy_count,
        })
    }

    fn domain_exists(&self, domain: &DomainName) -> bool {
        self.domains
            .lock()
            .expect("domain registry mutex should not be poisoned")
            .iter()
            .any(|registered| registered == domain)
    }

    fn delegation_for_name(&self, name: &DomainName) -> Option<RegisteredDelegation> {
        self.delegations
            .lock()
            .expect("delegation registry mutex should not be poisoned")
            .iter()
            .find(|delegation| delegation.fully_qualified_name() == *name)
            .cloned()
    }

    fn projection_enabled(&self, domain: &DomainName, requested_scope: ProjectionScope) -> bool {
        concrete_scopes(requested_scope)
            .into_iter()
            .all(|scope| self.concrete_projection_enabled(domain, scope))
    }

    fn concrete_projection_enabled(
        &self,
        domain: &DomainName,
        requested_scope: ProjectionScope,
    ) -> bool {
        self.policy
            .lock()
            .expect("policy mutex should not be poisoned")
            .projections
            .iter()
            .rev()
            .find(|policy| policy_matches(policy, domain, requested_scope))
            .is_none_or(|policy| policy.directive == ProjectionDirective::Enable)
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

fn domain_is_under_root(domain: &DomainName, root: &DomainName) -> bool {
    domain == root || domain.as_str().ends_with(&format!(".{}", root.as_str()))
}

fn includes_public_records(scope: ProjectionScope) -> bool {
    matches!(
        scope,
        ProjectionScope::PublicRecords | ProjectionScope::Everything
    )
}

fn includes_redirect_rules(scope: ProjectionScope) -> bool {
    matches!(
        scope,
        ProjectionScope::RedirectRules | ProjectionScope::Everything
    )
}

fn concrete_scopes(scope: ProjectionScope) -> Vec<ProjectionScope> {
    match scope {
        ProjectionScope::PublicRecords => vec![ProjectionScope::PublicRecords],
        ProjectionScope::RedirectRules => vec![ProjectionScope::RedirectRules],
        ProjectionScope::Everything => vec![
            ProjectionScope::PublicRecords,
            ProjectionScope::RedirectRules,
        ],
    }
}

fn policy_matches(
    policy: &ProjectionPolicy,
    domain: &DomainName,
    requested_scope: ProjectionScope,
) -> bool {
    policy.domain == *domain
        && (policy.scope == ProjectionScope::Everything || policy.scope == requested_scope)
}
