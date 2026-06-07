//! Criome domain registry and projection runtime.
//!
//! The daemon owns provider-neutral domain meaning. Provider execution stays in
//! the `cloud` component.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use meta_signal_domain_criome::schema::lib as meta_schema;
use meta_signal_domain_criome::{
    Delegation as MetaDelegation, DomainDelegated, DomainRegistered, DomainRetired,
    Operation as MetaOperation, PolicySet, ProjectionDeclaration, ProjectionDirective,
    ProjectionPolicy, ProjectionSet, Registration, Reply as MetaReply,
    RequestRejected as MetaRequestRejected, Retirement,
};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_domain_criome::schema::lib as ordinary_schema;
use signal_domain_criome::{
    Address, Delegation, DelegationListing, DelegationQuery, DomainListing, DomainName,
    DomainNameSystemRecord, DomainQuery, NetworkAddress, Observation, ObservationResult,
    Operation as DomainOperation, Projection, ProjectionQuery, ProjectionScope, RecordKind,
    RedirectRule, Reply as DomainReply, RequestRejected as DomainRequestRejected, ResolutionQuery,
    ResolutionResult,
};
use signal_frame::{NonEmpty, Reply as FrameReply, SubReply};

use crate::schema_bridge::{
    SchemaMetaInput, SchemaMetaOutput, SchemaOrdinaryInput, SchemaOrdinaryOutput,
};

pub mod client;
pub mod daemon;
pub mod daemon_command;
pub mod schema;
mod schema_bridge;
pub mod schema_daemon;

pub use daemon_command::{DomainCriomeDaemonCommand, DomainCriomeDaemonConfigurationFile};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("signal frame error: {0}")]
    Frame(#[from] signal_frame::FrameError),

    #[error("length-prefixed frame error: {0}")]
    LengthPrefixedFrame(#[from] triad_runtime::FrameError),

    #[error("ordinary schema frame error: {0}")]
    OrdinarySchemaFrame(ordinary_schema::SignalFrameError),

    #[error("meta schema frame error: {0}")]
    MetaSchemaFrame(meta_schema::SignalFrameError),

    #[error("command-line route error: {0}")]
    CommandLineRoute(#[from] signal_frame::CommandLineRouteError),

    #[error("NOTA decode error: {0}")]
    Nota(#[from] nota_next::NotaDecodeError),

    #[error("command-line request error: {0}")]
    CommandLine(#[from] signal_frame::CommandLineError),

    #[error("configuration archive decode failed")]
    ConfigurationArchiveDecode,

    #[error("configuration archive encode failed")]
    ConfigurationArchiveEncode,

    #[error("configuration read failed at {path}: {source}")]
    ConfigurationRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("configuration write failed at {path}: {source}")]
    ConfigurationWrite {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("argument: {0}")]
    Argument(#[from] triad_runtime::ArgumentError),

    #[error("expected exactly one argument")]
    ExpectedSingleArgument,

    #[error("flag-style arguments are not part of component binaries: {0}")]
    FlagArgument(String),

    #[error("unexpected signal frame for this socket")]
    UnexpectedFrame,

    #[error("request read timed out")]
    RequestReadTimedOut,

    #[error("trailing input after single NOTA request")]
    TrailingInput,

    #[error("connection closed before a complete frame arrived")]
    ConnectionClosed,

    #[error("signal request was rejected before execution")]
    SignalRequestRejected,

    #[error("signal request failed during execution")]
    SignalRequestFailed,

    #[error("domain-criome store mutex was poisoned")]
    StorePoisoned,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn command_line_route(error: signal_frame::CommandLineRouteError) -> Self {
        Self::CommandLineRoute(error)
    }
}

impl From<ordinary_schema::SignalFrameError> for Error {
    fn from(error: ordinary_schema::SignalFrameError) -> Self {
        Self::OrdinarySchemaFrame(error)
    }
}

impl From<meta_schema::SignalFrameError> for Error {
    fn from(error: meta_schema::SignalFrameError) -> Self {
        Self::MetaSchemaFrame(error)
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfiguration {
    pub ordinary_socket_path: String,
    pub ordinary_socket_mode: u32,
    pub meta_socket_path: String,
    pub meta_socket_mode: u32,
}

impl DaemonConfiguration {
    pub fn from_rkyv_bytes(bytes: &[u8]) -> Result<Self> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
            .map_err(|_| Error::ConfigurationArchiveDecode)
    }

    pub fn to_rkyv_bytes(&self) -> Result<Vec<u8>> {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map_err(|_| Error::ConfigurationArchiveEncode)?;
        Ok(bytes.into_vec())
    }
}

impl triad_runtime::DaemonConfiguration for DaemonConfiguration {
    fn socket_path(&self) -> &Path {
        Path::new(&self.ordinary_socket_path)
    }

    fn meta_socket_path(&self) -> Option<&Path> {
        Some(Path::new(&self.meta_socket_path))
    }

    fn database_path(&self) -> &Path {
        Path::new("")
    }

    fn meta_socket_mode(&self) -> Option<triad_runtime::SocketMode> {
        Some(triad_runtime::SocketMode::new(self.meta_socket_mode))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionState {
    domain: DomainName,
    records: Vec<DomainNameSystemRecord>,
    redirects: Vec<RedirectRule>,
}

impl ProjectionState {
    pub fn from_declaration(declaration: ProjectionDeclaration) -> Self {
        Self {
            domain: declaration.domain,
            records: declaration.records,
            redirects: declaration.redirects,
        }
    }

    pub fn into_declaration(self) -> ProjectionDeclaration {
        ProjectionDeclaration {
            domain: self.domain,
            records: self.records,
            redirects: self.redirects,
        }
    }

    pub fn project(&self, query: ProjectionQuery) -> Projection {
        Projection {
            records: self.records_for_scope(query.scope),
            redirects: self.redirects_for_scope(query.scope),
            query,
        }
    }

    pub fn resolution_addresses(&self, query: &ResolutionQuery) -> Vec<Address> {
        self.records
            .iter()
            .filter(|record| record.name == query.name)
            .filter_map(|record| AddressProjection::from_record(record).into_address())
            .collect()
    }

    fn records_for_scope(&self, scope: ProjectionScope) -> Vec<DomainNameSystemRecord> {
        match scope {
            ProjectionScope::PublicRecords | ProjectionScope::Everything => self.records.clone(),
            ProjectionScope::RedirectRules => Vec::new(),
        }
    }

    fn redirects_for_scope(&self, scope: ProjectionScope) -> Vec<RedirectRule> {
        match scope {
            ProjectionScope::RedirectRules | ProjectionScope::Everything => self.redirects.clone(),
            ProjectionScope::PublicRecords => Vec::new(),
        }
    }
}

pub struct AddressProjection<'record> {
    record: &'record DomainNameSystemRecord,
}

impl<'record> AddressProjection<'record> {
    pub fn from_record(record: &'record DomainNameSystemRecord) -> Self {
        Self { record }
    }

    pub fn into_address(self) -> Option<Address> {
        match self.record.kind {
            RecordKind::AddressV4 | RecordKind::AddressV6 => Some(Address {
                name: self.record.name.clone(),
                address: NetworkAddress::new(self.record.value.as_str()),
            }),
            RecordKind::CanonicalName | RecordKind::Text => None,
        }
    }
}

#[derive(Debug)]
pub struct Store {
    domains: Mutex<Vec<DomainName>>,
    delegations: Mutex<Vec<MetaDelegation>>,
    policy: Mutex<meta_signal_domain_criome::Policy>,
    projections: Mutex<Vec<ProjectionState>>,
}

impl Store {
    pub fn new() -> Self {
        Self {
            domains: Mutex::new(Vec::new()),
            delegations: Mutex::new(Vec::new()),
            policy: Mutex::new(meta_signal_domain_criome::Policy {
                projections: Vec::new(),
            }),
            projections: Mutex::new(Vec::new()),
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

    pub fn handle_meta_request(
        &self,
        request: meta_signal_domain_criome::ChannelRequest,
    ) -> meta_signal_domain_criome::ChannelReply {
        let replies = request
            .payloads
            .into_iter()
            .map(|operation| SubReply::Ok(self.handle_meta_operation(operation)))
            .collect::<Vec<_>>();
        FrameReply::committed(
            NonEmpty::try_from_vec(replies).expect("signal request is guaranteed non-empty"),
        )
    }

    pub fn handle_ordinary_input(&self, input: ordinary_schema::Input) -> ordinary_schema::Output {
        match SchemaOrdinaryInput::new(input).into_operation() {
            Some(operation) => {
                SchemaOrdinaryOutput::new(self.handle_ordinary_operation(operation)).into_output()
            }
            None => ordinary_schema::Output::Validated(ordinary_schema::ValidationReport::new(
                Vec::new(),
            )),
        }
    }

    pub fn handle_meta_input(&self, input: meta_schema::Input) -> meta_schema::Output {
        let operation = SchemaMetaInput::new(input).into_operation();
        SchemaMetaOutput::new(self.handle_meta_operation(operation)).into_output()
    }

    fn handle_ordinary_operation(&self, operation: DomainOperation) -> DomainReply {
        match operation {
            DomainOperation::Observe(observation) => self.observe(observation),
            DomainOperation::Resolve(query) => self.resolve(query),
            DomainOperation::Project(query) => self.project(query),
        }
    }

    fn handle_meta_operation(&self, operation: MetaOperation) -> MetaReply {
        match operation {
            MetaOperation::RegisterDomain(registration) => self.register_domain(registration),
            MetaOperation::Delegate(delegation) => self.delegate(delegation),
            MetaOperation::RetireDomain(retirement) => self.retire_domain(retirement),
            MetaOperation::SetPolicy(policy) => self.set_policy(policy),
            MetaOperation::SetProjection(declaration) => self.set_projection(declaration),
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
            Observation::Projection(query) => self.project(query),
        }
    }

    fn resolve(&self, query: ResolutionQuery) -> DomainReply {
        if !self.domain_is_registered(&query.name) {
            return DomainReply::RequestRejected(DomainRequestRejected {
                operation: signal_domain_criome::OperationKind::Resolve,
                reason: signal_domain_criome::RejectionReason::DomainUnknown,
            });
        }
        let addresses = self
            .projection_for_domain(&query.name)
            .map(|projection| projection.resolution_addresses(&query))
            .unwrap_or_default();
        DomainReply::Resolved(ResolutionResult { query, addresses })
    }

    fn project(&self, query: ProjectionQuery) -> DomainReply {
        if !self.domain_is_registered(&query.domain) {
            return DomainReply::RequestRejected(DomainRequestRejected {
                operation: signal_domain_criome::OperationKind::Project,
                reason: signal_domain_criome::RejectionReason::DomainUnknown,
            });
        }
        if !self.projection_is_enabled(&query.domain, query.scope) {
            return DomainReply::RequestRejected(DomainRequestRejected {
                operation: signal_domain_criome::OperationKind::Project,
                reason: signal_domain_criome::RejectionReason::ProjectionUnavailable,
            });
        }
        match self.projection_for_domain(&query.domain) {
            Some(projection) => DomainReply::Projected(projection.project(query)),
            None => DomainReply::RequestRejected(DomainRequestRejected {
                operation: signal_domain_criome::OperationKind::Project,
                reason: signal_domain_criome::RejectionReason::ProjectionUnavailable,
            }),
        }
    }

    fn domains(&self, query: DomainQuery) -> DomainListing {
        let domains = self
            .domains
            .lock()
            .expect("domains mutex")
            .iter()
            .filter(|domain| {
                query
                    .root
                    .as_ref()
                    .is_none_or(|root| DomainRoot::new(root).contains(domain))
            })
            .cloned()
            .collect();
        DomainListing { domains }
    }

    fn delegations(&self, query: DelegationQuery) -> DelegationListing {
        let delegations = self
            .delegations
            .lock()
            .expect("delegations mutex")
            .iter()
            .filter(|delegation| {
                query
                    .domain
                    .as_ref()
                    .is_none_or(|domain| &delegation.domain == domain)
            })
            .map(DelegationView::from)
            .map(Delegation::from)
            .collect();
        DelegationListing { delegations }
    }

    fn register_domain(&self, registration: Registration) -> MetaReply {
        let mut domains = self.domains.lock().expect("domains mutex");
        if domains.iter().any(|domain| domain == &registration.domain) {
            return MetaReply::RequestRejected(MetaRequestRejected {
                operation: meta_signal_domain_criome::OperationKind::RegisterDomain,
                reason: meta_signal_domain_criome::RejectionReason::DomainAlreadyRegistered,
            });
        }
        domains.push(registration.domain.clone());
        MetaReply::DomainRegistered(DomainRegistered {
            domain: registration.domain,
        })
    }

    fn delegate(&self, delegation: MetaDelegation) -> MetaReply {
        if !self.domain_is_registered(&delegation.domain) {
            return MetaReply::RequestRejected(MetaRequestRejected {
                operation: meta_signal_domain_criome::OperationKind::Delegate,
                reason: meta_signal_domain_criome::RejectionReason::DomainUnknown,
            });
        }
        let mut delegations = self.delegations.lock().expect("delegations mutex");
        if delegations.iter().any(|existing| {
            existing.domain == delegation.domain && existing.name == delegation.name
        }) {
            return MetaReply::RequestRejected(MetaRequestRejected {
                operation: meta_signal_domain_criome::OperationKind::Delegate,
                reason: meta_signal_domain_criome::RejectionReason::DelegationAlreadyExists,
            });
        }
        delegations.push(delegation.clone());
        MetaReply::DomainDelegated(DomainDelegated {
            name: delegation.name,
            domain: delegation.domain,
        })
    }

    fn retire_domain(&self, retirement: Retirement) -> MetaReply {
        if !self.domain_is_registered(&retirement.domain) {
            return MetaReply::RequestRejected(MetaRequestRejected {
                operation: meta_signal_domain_criome::OperationKind::RetireDomain,
                reason: meta_signal_domain_criome::RejectionReason::DomainUnknown,
            });
        }
        self.domains
            .lock()
            .expect("domains mutex")
            .retain(|domain| domain != &retirement.domain);
        self.delegations
            .lock()
            .expect("delegations mutex")
            .retain(|delegation| delegation.domain != retirement.domain);
        self.projections
            .lock()
            .expect("projections mutex")
            .retain(|projection| projection.domain != retirement.domain);
        self.policy
            .lock()
            .expect("policy mutex")
            .projections
            .retain(|policy| policy.domain != retirement.domain);
        MetaReply::DomainRetired(DomainRetired {
            domain: retirement.domain,
        })
    }

    fn set_policy(&self, policy: meta_signal_domain_criome::Policy) -> MetaReply {
        let projection_policy_count = policy.projections.len() as u64;
        *self.policy.lock().expect("policy mutex") = policy;
        MetaReply::PolicySet(PolicySet {
            projection_policy_count,
        })
    }

    fn set_projection(&self, declaration: ProjectionDeclaration) -> MetaReply {
        if !self.domain_is_registered(&declaration.domain) {
            return MetaReply::RequestRejected(MetaRequestRejected {
                operation: meta_signal_domain_criome::OperationKind::SetProjection,
                reason: meta_signal_domain_criome::RejectionReason::DomainUnknown,
            });
        }
        let domain = declaration.domain.clone();
        let record_count = declaration.records.len() as u64;
        let redirect_count = declaration.redirects.len() as u64;
        let state = ProjectionState::from_declaration(declaration);
        let mut projections = self.projections.lock().expect("projections mutex");
        if let Some(existing) = projections
            .iter_mut()
            .find(|projection| projection.domain == domain)
        {
            *existing = state;
        } else {
            projections.push(state);
        }
        MetaReply::ProjectionSet(ProjectionSet {
            domain,
            record_count,
            redirect_count,
        })
    }

    fn domain_is_registered(&self, domain: &DomainName) -> bool {
        self.domains
            .lock()
            .expect("domains mutex")
            .iter()
            .any(|registered| registered == domain)
    }

    fn projection_for_domain(&self, domain: &DomainName) -> Option<ProjectionState> {
        self.projections
            .lock()
            .expect("projections mutex")
            .iter()
            .find(|projection| &projection.domain == domain)
            .cloned()
    }

    fn projection_is_enabled(&self, domain: &DomainName, scope: ProjectionScope) -> bool {
        ProjectionPolicySet::new(
            self.policy
                .lock()
                .expect("policy mutex")
                .projections
                .clone(),
        )
        .allows(domain, scope)
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DomainRoot<'root> {
    root: &'root DomainName,
}

impl<'root> DomainRoot<'root> {
    pub fn new(root: &'root DomainName) -> Self {
        Self { root }
    }

    pub fn contains(&self, domain: &DomainName) -> bool {
        domain == self.root || domain.as_str().ends_with(self.root.as_str())
    }
}

pub struct DelegationView<'delegation> {
    delegation: &'delegation MetaDelegation,
}

impl<'delegation> From<&'delegation MetaDelegation> for DelegationView<'delegation> {
    fn from(delegation: &'delegation MetaDelegation) -> Self {
        Self { delegation }
    }
}

impl<'delegation> From<DelegationView<'delegation>> for Delegation {
    fn from(view: DelegationView<'delegation>) -> Self {
        Self {
            name: view.delegation.name.clone(),
            domain: view.delegation.domain.clone(),
            target: view.delegation.target.clone(),
        }
    }
}

pub struct ProjectionPolicySet {
    policies: Vec<ProjectionPolicy>,
}

impl ProjectionPolicySet {
    pub fn new(policies: Vec<ProjectionPolicy>) -> Self {
        Self { policies }
    }

    pub fn allows(&self, domain: &DomainName, scope: ProjectionScope) -> bool {
        self.policies
            .iter()
            .filter(|policy| &policy.domain == domain)
            .rfind(|policy| ProjectionScopeMatch::new(policy.scope, scope).matches())
            .is_some_and(|policy| policy.directive == ProjectionDirective::Enable)
    }
}

pub struct ProjectionScopeMatch {
    policy: ProjectionScope,
    requested: ProjectionScope,
}

impl ProjectionScopeMatch {
    pub fn new(policy: ProjectionScope, requested: ProjectionScope) -> Self {
        Self { policy, requested }
    }

    pub fn matches(&self) -> bool {
        self.policy == ProjectionScope::Everything || self.policy == self.requested
    }
}
