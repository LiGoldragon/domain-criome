//! Criome domain registry and projection runtime.
//!
//! The daemon owns provider-neutral domain meaning. Provider execution stays in
//! the `cloud` component.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use meta_signal_domain_criome::schema::lib as meta_schema;
use meta_signal_domain_criome::{
    Delegation as MetaDelegation, DomainDelegated, DomainRegistered, DomainRetired,
    Operation as MetaOperation, Policy, PolicySet, ProjectionDeclaration, ProjectionDirective,
    ProjectionPolicy, ProjectionSet, Registration, Reply as MetaReply,
    RequestRejected as MetaRequestRejected, Retirement,
};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use sema_engine::{
    CommitRequest, Engine, EngineOpen, EngineRecord, FamilyName, QueryPlan, RecordKey, SchemaHash,
    SchemaVersion, TableDescriptor, TableName, TableReference, VersionedStoreName,
    VersioningPolicy,
};
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

const DOMAIN_CRIOME_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1);
const DOMAIN_CRIOME_STATE_TABLE: TableName = TableName::new("domain-criome.state");
const DOMAIN_CRIOME_STATE_FAMILY: &str = "domain-criome-state";
const POLICY_RECORD_KEY: &str = "policy";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("signal frame error: {0}")]
    Frame(#[from] signal_frame::FrameError),

    #[error("length-prefixed frame error: {0}")]
    LengthPrefixedFrame(#[from] triad_runtime::FrameError),

    #[error("engine request error: {0}")]
    EngineRequest(#[from] triad_runtime::EngineRequestError),

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

    #[error("domain-criome sema engine: {0}")]
    SemaEngine(#[from] sema_engine::Error),

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
    pub database_path: String,
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

impl triad_runtime::BindingSurface for DaemonConfiguration {
    fn socket_path(&self) -> &Path {
        Path::new(&self.ordinary_socket_path)
    }

    fn meta_socket_path(&self) -> Option<&Path> {
        Some(Path::new(&self.meta_socket_path))
    }

    fn database_path(&self) -> &Path {
        Path::new(&self.database_path)
    }

    fn meta_socket_mode(&self) -> Option<triad_runtime::SocketMode> {
        Some(triad_runtime::SocketMode::new(self.meta_socket_mode))
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
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

pub struct Store {
    domains: Mutex<Vec<DomainName>>,
    delegations: Mutex<Vec<MetaDelegation>>,
    policy: Mutex<Policy>,
    projections: Mutex<Vec<ProjectionState>>,
    tables: Option<DomainTables>,
}

impl Store {
    pub fn new() -> Self {
        Self {
            domains: Mutex::new(Vec::new()),
            delegations: Mutex::new(Vec::new()),
            policy: Mutex::new(Policy {
                projections: Vec::new(),
            }),
            projections: Mutex::new(Vec::new()),
            tables: None,
        }
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let tables = DomainTables::open(path.as_ref())?;
        let mut store = Self::new();
        store.load_materialized_records(tables.records()?);
        store.tables = Some(tables);
        Ok(store)
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
            .map(|operation| {
                SubReply::Ok(
                    self.try_handle_meta_operation(operation)
                        .expect("domain-criome meta operation"),
                )
            })
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
        self.try_handle_meta_input(input)
            .expect("domain-criome handles meta input")
    }

    pub fn try_handle_meta_input(&self, input: meta_schema::Input) -> Result<meta_schema::Output> {
        let operation = SchemaMetaInput::new(input).into_operation();
        Ok(SchemaMetaOutput::new(self.try_handle_meta_operation(operation)?).into_output())
    }

    fn handle_ordinary_operation(&self, operation: DomainOperation) -> DomainReply {
        match operation {
            DomainOperation::Observe(observation) => self.observe(observation),
            DomainOperation::Resolve(query) => self.resolve(query),
            DomainOperation::Project(query) => self.project(query),
        }
    }

    fn try_handle_meta_operation(&self, operation: MetaOperation) -> Result<MetaReply> {
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

    fn register_domain(&self, registration: Registration) -> Result<MetaReply> {
        if self.domain_is_registered(&registration.domain) {
            return Ok(MetaReply::RequestRejected(MetaRequestRejected {
                operation: meta_signal_domain_criome::OperationKind::RegisterDomain,
                reason: meta_signal_domain_criome::RejectionReason::DomainAlreadyRegistered,
            }));
        }
        self.persist_upserts(vec![StoredStateRecord::registered_domain(
            registration.domain.clone(),
        )])?;
        let mut domains = self.domains.lock().expect("domains mutex");
        domains.push(registration.domain.clone());
        Ok(MetaReply::DomainRegistered(DomainRegistered {
            domain: registration.domain,
        }))
    }

    fn delegate(&self, delegation: MetaDelegation) -> Result<MetaReply> {
        if !self.domain_is_registered(&delegation.domain) {
            return Ok(MetaReply::RequestRejected(MetaRequestRejected {
                operation: meta_signal_domain_criome::OperationKind::Delegate,
                reason: meta_signal_domain_criome::RejectionReason::DomainUnknown,
            }));
        }
        if self
            .delegations
            .lock()
            .expect("delegations mutex")
            .iter()
            .any(|existing| {
                existing.domain == delegation.domain && existing.name == delegation.name
            })
        {
            return Ok(MetaReply::RequestRejected(MetaRequestRejected {
                operation: meta_signal_domain_criome::OperationKind::Delegate,
                reason: meta_signal_domain_criome::RejectionReason::DelegationAlreadyExists,
            }));
        }
        self.persist_upserts(vec![StoredStateRecord::delegation(delegation.clone())])?;
        let mut delegations = self.delegations.lock().expect("delegations mutex");
        delegations.push(delegation.clone());
        Ok(MetaReply::DomainDelegated(DomainDelegated {
            name: delegation.name,
            domain: delegation.domain,
        }))
    }

    fn retire_domain(&self, retirement: Retirement) -> Result<MetaReply> {
        if !self.domain_is_registered(&retirement.domain) {
            return Ok(MetaReply::RequestRejected(MetaRequestRejected {
                operation: meta_signal_domain_criome::OperationKind::RetireDomain,
                reason: meta_signal_domain_criome::RejectionReason::DomainUnknown,
            }));
        }
        let mut policy = self.policy.lock().expect("policy mutex").clone();
        policy
            .projections
            .retain(|policy| policy.domain != retirement.domain);
        self.persist_retirement(&retirement.domain, policy.clone())?;
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
        *self.policy.lock().expect("policy mutex") = policy;
        Ok(MetaReply::DomainRetired(DomainRetired {
            domain: retirement.domain,
        }))
    }

    fn set_policy(&self, policy: Policy) -> Result<MetaReply> {
        let projection_policy_count = policy.projections.len() as u64;
        self.persist_upserts(vec![StoredStateRecord::policy(policy.clone())])?;
        *self.policy.lock().expect("policy mutex") = policy;
        Ok(MetaReply::PolicySet(PolicySet {
            projection_policy_count,
        }))
    }

    fn set_projection(&self, declaration: ProjectionDeclaration) -> Result<MetaReply> {
        if !self.domain_is_registered(&declaration.domain) {
            return Ok(MetaReply::RequestRejected(MetaRequestRejected {
                operation: meta_signal_domain_criome::OperationKind::SetProjection,
                reason: meta_signal_domain_criome::RejectionReason::DomainUnknown,
            }));
        }
        let domain = declaration.domain.clone();
        let record_count = declaration.records.len() as u64;
        let redirect_count = declaration.redirects.len() as u64;
        let state = ProjectionState::from_declaration(declaration);
        self.persist_upserts(vec![StoredStateRecord::projection(state.clone())])?;
        let mut projections = self.projections.lock().expect("projections mutex");
        if let Some(existing) = projections
            .iter_mut()
            .find(|projection| projection.domain == domain)
        {
            *existing = state;
        } else {
            projections.push(state);
        }
        Ok(MetaReply::ProjectionSet(ProjectionSet {
            domain,
            record_count,
            redirect_count,
        }))
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

    fn load_materialized_records(&self, records: Vec<StoredStateRecord>) {
        for record in records {
            match record {
                StoredStateRecord::RegisteredDomain(domain) => {
                    self.domains.lock().expect("domains mutex").push(domain);
                }
                StoredStateRecord::Delegation(delegation) => {
                    self.delegations
                        .lock()
                        .expect("delegations mutex")
                        .push(delegation);
                }
                StoredStateRecord::Policy(policy) => {
                    *self.policy.lock().expect("policy mutex") = policy;
                }
                StoredStateRecord::Projection(projection) => {
                    self.projections
                        .lock()
                        .expect("projections mutex")
                        .push(projection);
                }
            }
        }
    }

    fn persist_upserts(&self, records: Vec<StoredStateRecord>) -> Result<()> {
        if let Some(tables) = self.tables.as_ref() {
            tables.commit_upserts(records)?;
        }
        Ok(())
    }

    fn persist_retirement(&self, domain: &DomainName, policy: Policy) -> Result<()> {
        if let Some(tables) = self.tables.as_ref() {
            tables.retire_domain(domain, policy)?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for Store {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Store")
            .field(
                "domain_count",
                &self.domains.lock().expect("domains mutex").len(),
            )
            .field(
                "delegation_count",
                &self.delegations.lock().expect("delegations mutex").len(),
            )
            .field(
                "projection_count",
                &self.projections.lock().expect("projections mutex").len(),
            )
            .field("durable", &self.tables.is_some())
            .finish()
    }
}

pub struct DomainTables {
    engine: Engine,
    records: TableReference<StoredStateRecord>,
}

impl DomainTables {
    pub fn open(path: &Path) -> Result<Self> {
        let mut engine = Engine::open(Self::engine_open(path))?;
        let records = engine.register_table(Self::state_descriptor())?;
        Ok(Self { engine, records })
    }

    fn engine_open(path: &Path) -> EngineOpen {
        EngineOpen::new(path.to_path_buf(), DOMAIN_CRIOME_SCHEMA_VERSION)
            .with_versioning(Self::versioning_policy())
    }

    fn versioning_policy() -> VersioningPolicy {
        VersioningPolicy::new(VersionedStoreName::new("domain-criome"))
    }

    fn state_descriptor() -> TableDescriptor<StoredStateRecord> {
        TableDescriptor::new(
            DOMAIN_CRIOME_STATE_TABLE,
            FamilyName::new(DOMAIN_CRIOME_STATE_FAMILY),
            SchemaHash::for_label(format!(
                "{DOMAIN_CRIOME_STATE_FAMILY}-v{}",
                DOMAIN_CRIOME_SCHEMA_VERSION.value()
            )),
        )
    }

    fn records(&self) -> Result<Vec<StoredStateRecord>> {
        Ok(self
            .engine
            .match_records(QueryPlan::all(self.records))?
            .records()
            .to_vec())
    }

    fn record(&self, record: &StoredStateRecord) -> Result<Option<StoredStateRecord>> {
        Ok(self
            .engine
            .match_records(QueryPlan::key(self.records, record.record_key()))?
            .records()
            .first()
            .cloned())
    }

    fn commit_upserts(&self, records: Vec<StoredStateRecord>) -> Result<()> {
        let mut request = CommitRequest::new(self.records);
        for record in records {
            if self.record(&record)?.is_some() {
                request = request.mutate(record);
            } else {
                request = request.assert(record);
            }
        }
        if request.operation_count() > 0 {
            self.engine.commit(request)?;
        }
        Ok(())
    }

    fn retire_domain(&self, domain: &DomainName, policy: Policy) -> Result<()> {
        let mut request = CommitRequest::new(self.records);
        for record in self.records()? {
            if record.belongs_to_domain(domain) {
                request = request.retract(record.record_key());
            }
        }
        let policy = StoredStateRecord::policy(policy);
        if self.record(&policy)?.is_some() {
            request = request.mutate(policy);
        } else {
            request = request.assert(policy);
        }
        if request.operation_count() > 0 {
            self.engine.commit(request)?;
        }
        Ok(())
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum StoredStateRecord {
    RegisteredDomain(DomainName),
    Delegation(MetaDelegation),
    Policy(Policy),
    Projection(ProjectionState),
}

impl StoredStateRecord {
    pub fn registered_domain(domain: DomainName) -> Self {
        Self::RegisteredDomain(domain)
    }

    pub fn delegation(delegation: MetaDelegation) -> Self {
        Self::Delegation(delegation)
    }

    pub fn policy(policy: Policy) -> Self {
        Self::Policy(policy)
    }

    pub fn projection(projection: ProjectionState) -> Self {
        Self::Projection(projection)
    }

    fn belongs_to_domain(&self, domain: &DomainName) -> bool {
        match self {
            Self::RegisteredDomain(record_domain) => record_domain == domain,
            Self::Delegation(delegation) => &delegation.domain == domain,
            Self::Policy(_) => false,
            Self::Projection(projection) => &projection.domain == domain,
        }
    }

    fn key_string(&self) -> String {
        match self {
            Self::RegisteredDomain(domain) => format!("domain:{}", domain.as_str()),
            Self::Delegation(delegation) => format!(
                "delegation:{}:{}",
                delegation.domain.as_str(),
                delegation.name.as_str()
            ),
            Self::Policy(_) => POLICY_RECORD_KEY.to_owned(),
            Self::Projection(projection) => format!("projection:{}", projection.domain.as_str()),
        }
    }
}

impl EngineRecord for StoredStateRecord {
    fn record_key(&self) -> RecordKey {
        RecordKey::new(self.key_string())
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
