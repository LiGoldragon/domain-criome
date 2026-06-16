use meta_signal_domain_criome::schema::lib as meta_schema;
use meta_signal_domain_criome::{
    Delegation as MetaDelegation, Operation as MetaOperation, ProjectionDeclaration,
    ProjectionDirective, ProjectionPolicy, Registration, Reply as MetaReply,
    RequestRejected as MetaRequestRejected, Retirement,
};
use signal_domain_criome::schema::lib as ordinary_schema;
use signal_domain_criome::{
    Delegation, DelegationListing, DelegationQuery, DomainListing, DomainName,
    DomainNameSystemRecord, DomainQuery, Observation, ObservationResult,
    Operation as DomainOperation, Projection, ProjectionQuery, ProjectionScope, RecordKind,
    RedirectRule, Reply as DomainReply, RequestRejected as DomainRequestRejected, ResolutionQuery,
    ResolutionResult,
};

pub(crate) struct SchemaOrdinaryInput {
    input: ordinary_schema::Input,
}

impl SchemaOrdinaryInput {
    pub fn new(input: ordinary_schema::Input) -> Self {
        Self { input }
    }

    pub fn from_operation(operation: DomainOperation) -> Self {
        let input = match operation {
            DomainOperation::Observe(observation) => {
                ordinary_schema::Input::Observe(LegacyObservation::new(observation).into_schema())
            }
            DomainOperation::Resolve(query) => {
                ordinary_schema::Input::Resolve(LegacyResolutionQuery::new(query).into_schema())
            }
            DomainOperation::Project(query) => {
                ordinary_schema::Input::Project(LegacyProjectionQuery::new(query).into_schema())
            }
        };
        Self { input }
    }

    pub fn into_input(self) -> ordinary_schema::Input {
        self.input
    }

    pub fn into_operation(self) -> Option<DomainOperation> {
        match self.input {
            ordinary_schema::Input::Observe(observation) => Some(DomainOperation::Observe(
                SchemaObservation::new(observation).into_legacy(),
            )),
            ordinary_schema::Input::Resolve(query) => Some(DomainOperation::Resolve(
                SchemaResolutionQuery::new(query).into_legacy(),
            )),
            ordinary_schema::Input::Project(query) => Some(DomainOperation::Project(
                SchemaProjectionQuery::new(query).into_legacy(),
            )),
            ordinary_schema::Input::Validate(_) => None,
        }
    }
}

pub(crate) struct SchemaOrdinaryOutput {
    reply: DomainReply,
}

impl SchemaOrdinaryOutput {
    pub fn new(reply: DomainReply) -> Self {
        Self { reply }
    }

    pub fn into_output(self) -> ordinary_schema::Output {
        match self.reply {
            DomainReply::Observed(result) => ordinary_schema::Output::Observed(
                LegacyObservationResult::new(result).into_schema(),
            ),
            DomainReply::Resolved(result) => {
                ordinary_schema::Output::Resolved(LegacyResolutionResult::new(result).into_schema())
            }
            DomainReply::Projected(projection) => {
                ordinary_schema::Output::Projected(LegacyProjection::new(projection).into_schema())
            }
            DomainReply::RequestRejected(rejection) => ordinary_schema::Output::RequestRejected(
                LegacyDomainRejection::new(rejection).into_schema(),
            ),
        }
    }
}

pub(crate) struct SchemaMetaInput {
    input: meta_schema::Input,
}

impl SchemaMetaInput {
    pub fn new(input: meta_schema::Input) -> Self {
        Self { input }
    }

    pub fn from_operation(operation: MetaOperation) -> Self {
        let input = match operation {
            MetaOperation::RegisterDomain(registration) => meta_schema::Input::RegisterDomain(
                meta_schema::Registration::new(registration.domain.as_str().to_owned().into()),
            ),
            MetaOperation::Delegate(delegation) => {
                meta_schema::Input::Delegate(LegacyMetaDelegation::new(delegation).into_schema())
            }
            MetaOperation::RetireDomain(retirement) => meta_schema::Input::RetireDomain(
                meta_schema::Retirement::new(retirement.domain.as_str().to_owned().into()),
            ),
            MetaOperation::SetPolicy(policy) => {
                meta_schema::Input::SetPolicy(LegacyPolicy::new(policy).into_schema())
            }
            MetaOperation::SetProjection(declaration) => meta_schema::Input::SetProjection(
                LegacyProjectionDeclaration::new(declaration).into_schema(),
            ),
        };
        Self { input }
    }

    pub fn into_input(self) -> meta_schema::Input {
        self.input
    }

    pub fn into_operation(self) -> MetaOperation {
        match self.input {
            meta_schema::Input::RegisterDomain(registration) => {
                MetaOperation::RegisterDomain(Registration {
                    domain: DomainName::new(registration.into_payload().into_payload()),
                })
            }
            meta_schema::Input::Delegate(delegation) => {
                MetaOperation::Delegate(SchemaMetaDelegation::new(delegation).into_legacy())
            }
            meta_schema::Input::RetireDomain(retirement) => {
                MetaOperation::RetireDomain(Retirement {
                    domain: DomainName::new(retirement.into_payload().into_payload()),
                })
            }
            meta_schema::Input::SetPolicy(policy) => {
                MetaOperation::SetPolicy(SchemaPolicy::new(policy).into_legacy())
            }
            meta_schema::Input::SetProjection(declaration) => MetaOperation::SetProjection(
                SchemaProjectionDeclaration::new(declaration).into_legacy(),
            ),
        }
    }
}

pub(crate) struct SchemaMetaOutput {
    reply: MetaReply,
}

impl SchemaMetaOutput {
    pub fn new(reply: MetaReply) -> Self {
        Self { reply }
    }

    pub fn into_output(self) -> meta_schema::Output {
        match self.reply {
            MetaReply::DomainRegistered(registered) => meta_schema::Output::DomainRegistered(
                meta_schema::DomainRegistered::new(registered.domain.as_str().to_owned().into()),
            ),
            MetaReply::DomainDelegated(delegated) => {
                meta_schema::Output::DelegationSet(meta_schema::DelegationSet {
                    delegation_name: delegated.name.as_str().to_owned().into(),
                    domain: delegated.domain.as_str().to_owned().into(),
                })
            }
            MetaReply::DomainRetired(retired) => meta_schema::Output::DomainRetired(
                meta_schema::DomainRetired::new(retired.domain.as_str().to_owned().into()),
            ),
            MetaReply::PolicySet(policy) => meta_schema::Output::PolicySet(
                meta_schema::PolicySet::new(policy.projection_policy_count),
            ),
            MetaReply::ProjectionSet(projection) => {
                meta_schema::Output::ProjectionSet(meta_schema::ProjectionSet {
                    domain: projection.domain.as_str().to_owned().into(),
                    record_count: projection.record_count,
                    redirect_count: projection.redirect_count,
                })
            }
            MetaReply::RequestRejected(rejection) => meta_schema::Output::RequestRejected(
                LegacyMetaRejection::new(rejection).into_schema(),
            ),
        }
    }
}

struct SchemaObservation {
    observation: ordinary_schema::Observation,
}

impl SchemaObservation {
    pub fn new(observation: ordinary_schema::Observation) -> Self {
        Self { observation }
    }

    pub fn into_legacy(self) -> Observation {
        match self.observation {
            ordinary_schema::Observation::Domains(query) => {
                Observation::Domains(SchemaDomainQuery::new(query.into_payload()).into_legacy())
            }
            ordinary_schema::Observation::Delegations(query) => Observation::Delegations(
                SchemaDelegationQuery::new(query.into_payload()).into_legacy(),
            ),
            ordinary_schema::Observation::Projection(query) => Observation::Projection(
                SchemaProjectionQuery::new(query.into_payload()).into_legacy(),
            ),
        }
    }
}

struct LegacyObservation {
    observation: Observation,
}

impl LegacyObservation {
    pub fn new(observation: Observation) -> Self {
        Self { observation }
    }

    pub fn into_schema(self) -> ordinary_schema::Observation {
        match self.observation {
            Observation::Domains(query) => ordinary_schema::Observation::Domains(
                LegacyDomainQuery::new(query).into_schema().into(),
            ),
            Observation::Delegations(query) => ordinary_schema::Observation::Delegations(
                LegacyDelegationQuery::new(query).into_schema().into(),
            ),
            Observation::Projection(query) => ordinary_schema::Observation::Projection(
                LegacyProjectionQuery::new(query).into_schema().into(),
            ),
        }
    }
}

struct SchemaDomainQuery {
    query: ordinary_schema::DomainQuery,
}

impl SchemaDomainQuery {
    pub fn new(query: ordinary_schema::DomainQuery) -> Self {
        Self { query }
    }

    pub fn into_legacy(self) -> DomainQuery {
        DomainQuery {
            root: self
                .query
                .into_payload()
                .map(|domain| DomainName::new(domain.into_payload())),
        }
    }
}

struct LegacyDomainQuery {
    query: DomainQuery,
}

impl LegacyDomainQuery {
    pub fn new(query: DomainQuery) -> Self {
        Self { query }
    }

    pub fn into_schema(self) -> ordinary_schema::DomainQuery {
        ordinary_schema::DomainQuery::new(
            self.query
                .root
                .map(|domain| domain.as_str().to_owned().into()),
        )
    }
}

struct SchemaDelegationQuery {
    query: ordinary_schema::DelegationQuery,
}

impl SchemaDelegationQuery {
    pub fn new(query: ordinary_schema::DelegationQuery) -> Self {
        Self { query }
    }

    pub fn into_legacy(self) -> DelegationQuery {
        DelegationQuery {
            domain: self
                .query
                .into_payload()
                .map(|domain| DomainName::new(domain.into_payload())),
        }
    }
}

struct LegacyDelegationQuery {
    query: DelegationQuery,
}

impl LegacyDelegationQuery {
    pub fn new(query: DelegationQuery) -> Self {
        Self { query }
    }

    pub fn into_schema(self) -> ordinary_schema::DelegationQuery {
        ordinary_schema::DelegationQuery::new(
            self.query
                .domain
                .map(|domain| domain.as_str().to_owned().into()),
        )
    }
}

struct SchemaResolutionQuery {
    query: ordinary_schema::ResolutionQuery,
}

impl SchemaResolutionQuery {
    pub fn new(query: ordinary_schema::ResolutionQuery) -> Self {
        Self { query }
    }

    pub fn into_legacy(self) -> ResolutionQuery {
        ResolutionQuery {
            name: DomainName::new(self.query.name.into_payload()),
            scope: SchemaResolutionScope::new(self.query.resolution_scope).into_legacy(),
        }
    }
}

struct LegacyResolutionQuery {
    query: ResolutionQuery,
}

impl LegacyResolutionQuery {
    pub fn new(query: ResolutionQuery) -> Self {
        Self { query }
    }

    pub fn into_schema(self) -> ordinary_schema::ResolutionQuery {
        ordinary_schema::ResolutionQuery {
            name: self.query.name.as_str().to_owned().into(),
            resolution_scope: LegacyResolutionScope::new(self.query.scope).into_schema(),
        }
    }
}

struct SchemaProjectionQuery {
    query: ordinary_schema::ProjectionQuery,
}

impl SchemaProjectionQuery {
    pub fn new(query: ordinary_schema::ProjectionQuery) -> Self {
        Self { query }
    }

    pub fn into_legacy(self) -> ProjectionQuery {
        ProjectionQuery {
            domain: DomainName::new(self.query.domain.into_payload()),
            scope: SchemaProjectionScope::new(self.query.projection_scope).into_legacy(),
        }
    }
}

struct LegacyProjectionQuery {
    query: ProjectionQuery,
}

impl LegacyProjectionQuery {
    pub fn new(query: ProjectionQuery) -> Self {
        Self { query }
    }

    pub fn into_schema(self) -> ordinary_schema::ProjectionQuery {
        ordinary_schema::ProjectionQuery {
            domain: self.query.domain.as_str().to_owned().into(),
            projection_scope: LegacyProjectionScope::new(self.query.scope).into_schema(),
        }
    }
}

struct SchemaResolutionScope {
    scope: ordinary_schema::ResolutionScope,
}

impl SchemaResolutionScope {
    pub fn new(scope: ordinary_schema::ResolutionScope) -> Self {
        Self { scope }
    }

    pub fn into_legacy(self) -> signal_domain_criome::ResolutionScope {
        match self.scope {
            ordinary_schema::ResolutionScope::Public => {
                signal_domain_criome::ResolutionScope::Public
            }
            ordinary_schema::ResolutionScope::Internal => {
                signal_domain_criome::ResolutionScope::Internal
            }
            ordinary_schema::ResolutionScope::Intelligent => {
                signal_domain_criome::ResolutionScope::Intelligent
            }
        }
    }
}

struct LegacyResolutionScope {
    scope: signal_domain_criome::ResolutionScope,
}

impl LegacyResolutionScope {
    pub fn new(scope: signal_domain_criome::ResolutionScope) -> Self {
        Self { scope }
    }

    pub fn into_schema(self) -> ordinary_schema::ResolutionScope {
        match self.scope {
            signal_domain_criome::ResolutionScope::Public => {
                ordinary_schema::ResolutionScope::Public
            }
            signal_domain_criome::ResolutionScope::Internal => {
                ordinary_schema::ResolutionScope::Internal
            }
            signal_domain_criome::ResolutionScope::Intelligent => {
                ordinary_schema::ResolutionScope::Intelligent
            }
        }
    }
}

struct SchemaProjectionScope {
    scope: ordinary_schema::ProjectionScope,
}

impl SchemaProjectionScope {
    pub fn new(scope: ordinary_schema::ProjectionScope) -> Self {
        Self { scope }
    }

    pub fn into_legacy(self) -> ProjectionScope {
        match self.scope {
            ordinary_schema::ProjectionScope::PublicRecords => ProjectionScope::PublicRecords,
            ordinary_schema::ProjectionScope::RedirectRules => ProjectionScope::RedirectRules,
            ordinary_schema::ProjectionScope::Everything => ProjectionScope::Everything,
        }
    }
}

struct LegacyProjectionScope {
    scope: ProjectionScope,
}

impl LegacyProjectionScope {
    pub fn new(scope: ProjectionScope) -> Self {
        Self { scope }
    }

    pub fn into_schema(self) -> ordinary_schema::ProjectionScope {
        match self.scope {
            ProjectionScope::PublicRecords => ordinary_schema::ProjectionScope::PublicRecords,
            ProjectionScope::RedirectRules => ordinary_schema::ProjectionScope::RedirectRules,
            ProjectionScope::Everything => ordinary_schema::ProjectionScope::Everything,
        }
    }

    pub fn into_meta_schema(self) -> meta_schema::ProjectionScope {
        match self.scope {
            ProjectionScope::PublicRecords => meta_schema::ProjectionScope::PublicRecords,
            ProjectionScope::RedirectRules => meta_schema::ProjectionScope::RedirectRules,
            ProjectionScope::Everything => meta_schema::ProjectionScope::Everything,
        }
    }
}

struct MetaSchemaProjectionScope {
    scope: meta_schema::ProjectionScope,
}

impl MetaSchemaProjectionScope {
    pub fn new(scope: meta_schema::ProjectionScope) -> Self {
        Self { scope }
    }

    pub fn into_legacy(self) -> ProjectionScope {
        match self.scope {
            meta_schema::ProjectionScope::PublicRecords => ProjectionScope::PublicRecords,
            meta_schema::ProjectionScope::RedirectRules => ProjectionScope::RedirectRules,
            meta_schema::ProjectionScope::Everything => ProjectionScope::Everything,
        }
    }
}

struct LegacyObservationResult {
    result: ObservationResult,
}

impl LegacyObservationResult {
    pub fn new(result: ObservationResult) -> Self {
        Self { result }
    }

    pub fn into_schema(self) -> ordinary_schema::ObservationResult {
        match self.result {
            ObservationResult::Domains(listing) => ordinary_schema::ObservationResult::Domains(
                LegacyDomainListing::new(listing).into_schema(),
            ),
            ObservationResult::Delegations(listing) => {
                ordinary_schema::ObservationResult::Delegations(
                    LegacyDelegationListing::new(listing).into_schema(),
                )
            }
            ObservationResult::Projection(projection) => {
                ordinary_schema::ObservationResult::Projection(
                    LegacyProjection::new(projection).into_schema(),
                )
            }
        }
    }
}

struct LegacyDomainListing {
    listing: DomainListing,
}

impl LegacyDomainListing {
    pub fn new(listing: DomainListing) -> Self {
        Self { listing }
    }

    pub fn into_schema(self) -> ordinary_schema::DomainListing {
        ordinary_schema::DomainListing::new(
            self.listing
                .domains
                .into_iter()
                .map(|domain| domain.as_str().to_owned().into())
                .collect(),
        )
    }
}

struct LegacyDelegationListing {
    listing: DelegationListing,
}

impl LegacyDelegationListing {
    pub fn new(listing: DelegationListing) -> Self {
        Self { listing }
    }

    pub fn into_schema(self) -> ordinary_schema::DelegationListing {
        ordinary_schema::DelegationListing::new(
            self.listing
                .delegations
                .into_iter()
                .map(|delegation| LegacyDelegation::new(delegation).into_schema())
                .collect(),
        )
    }
}

struct LegacyDelegation {
    delegation: Delegation,
}

impl LegacyDelegation {
    pub fn new(delegation: Delegation) -> Self {
        Self { delegation }
    }

    pub fn into_schema(self) -> ordinary_schema::Delegation {
        ordinary_schema::Delegation {
            delegation_name: self.delegation.name.as_str().to_owned().into(),
            domain_name: self.delegation.domain.as_str().to_owned().into(),
            delegation_target: self.delegation.target.as_str().to_owned().into(),
        }
    }
}

struct LegacyResolutionResult {
    result: ResolutionResult,
}

impl LegacyResolutionResult {
    pub fn new(result: ResolutionResult) -> Self {
        Self { result }
    }

    pub fn into_schema(self) -> ordinary_schema::ResolutionResult {
        ordinary_schema::ResolutionResult {
            query: LegacyResolutionQuery::new(self.result.query).into_schema(),
            addresses: self
                .result
                .addresses
                .into_iter()
                .map(|address| ordinary_schema::Address {
                    name: address.name.as_str().to_owned().into(),
                    address: address.address.as_str().to_owned().into(),
                })
                .collect(),
        }
    }
}

struct LegacyProjection {
    projection: Projection,
}

impl LegacyProjection {
    pub fn new(projection: Projection) -> Self {
        Self { projection }
    }

    pub fn into_schema(self) -> ordinary_schema::Projection {
        ordinary_schema::Projection {
            query: LegacyProjectionQuery::new(self.projection.query).into_schema(),
            records: self
                .projection
                .records
                .into_iter()
                .map(|record| LegacyRecord::new(record).into_schema())
                .collect(),
            redirects: self
                .projection
                .redirects
                .into_iter()
                .map(|redirect| LegacyRedirect::new(redirect).into_schema())
                .collect(),
        }
    }
}

struct LegacyRecord {
    record: DomainNameSystemRecord,
}

impl LegacyRecord {
    pub fn new(record: DomainNameSystemRecord) -> Self {
        Self { record }
    }

    pub fn into_schema(self) -> ordinary_schema::DomainNameSystemRecord {
        ordinary_schema::DomainNameSystemRecord {
            name: self.record.name.as_str().to_owned().into(),
            record_kind: LegacyRecordKind::new(self.record.kind).into_schema(),
            value: self.record.value.as_str().to_owned().into(),
        }
    }

    pub fn into_meta_schema(self) -> meta_schema::DomainNameSystemRecord {
        meta_schema::DomainNameSystemRecord {
            name: self.record.name.as_str().to_owned().into(),
            record_kind: LegacyRecordKind::new(self.record.kind).into_meta_schema(),
            value: self.record.value.as_str().to_owned().into(),
        }
    }
}

struct SchemaMetaRecord {
    record: meta_schema::DomainNameSystemRecord,
}

impl SchemaMetaRecord {
    pub fn new(record: meta_schema::DomainNameSystemRecord) -> Self {
        Self { record }
    }

    pub fn into_legacy(self) -> DomainNameSystemRecord {
        DomainNameSystemRecord {
            name: DomainName::new(self.record.name.into_payload()),
            kind: MetaSchemaRecordKind::new(self.record.record_kind).into_legacy(),
            value: signal_domain_criome::RecordValue::new(self.record.value.into_payload()),
        }
    }
}

struct LegacyRecordKind {
    kind: RecordKind,
}

impl LegacyRecordKind {
    pub fn new(kind: RecordKind) -> Self {
        Self { kind }
    }

    pub fn into_schema(self) -> ordinary_schema::RecordKind {
        match self.kind {
            RecordKind::AddressV4 => ordinary_schema::RecordKind::AddressV4,
            RecordKind::AddressV6 => ordinary_schema::RecordKind::AddressV6,
            RecordKind::CanonicalName => ordinary_schema::RecordKind::CanonicalName,
            RecordKind::Text => ordinary_schema::RecordKind::Text,
        }
    }

    pub fn into_meta_schema(self) -> meta_schema::RecordKind {
        match self.kind {
            RecordKind::AddressV4 => meta_schema::RecordKind::AddressV4,
            RecordKind::AddressV6 => meta_schema::RecordKind::AddressV6,
            RecordKind::CanonicalName => meta_schema::RecordKind::CanonicalName,
            RecordKind::Text => meta_schema::RecordKind::Text,
        }
    }
}

struct MetaSchemaRecordKind {
    kind: meta_schema::RecordKind,
}

impl MetaSchemaRecordKind {
    pub fn new(kind: meta_schema::RecordKind) -> Self {
        Self { kind }
    }

    pub fn into_legacy(self) -> RecordKind {
        match self.kind {
            meta_schema::RecordKind::AddressV4 => RecordKind::AddressV4,
            meta_schema::RecordKind::AddressV6 => RecordKind::AddressV6,
            meta_schema::RecordKind::CanonicalName => RecordKind::CanonicalName,
            meta_schema::RecordKind::Text => RecordKind::Text,
        }
    }
}

struct LegacyRedirect {
    redirect: RedirectRule,
}

impl LegacyRedirect {
    pub fn new(redirect: RedirectRule) -> Self {
        Self { redirect }
    }

    pub fn into_schema(self) -> ordinary_schema::RedirectRule {
        ordinary_schema::RedirectRule {
            source: self.redirect.source.as_str().to_owned().into(),
            target: self.redirect.target.as_str().to_owned().into(),
            redirect_status: LegacyRedirectStatus::new(self.redirect.status).into_schema(),
            path_treatment: LegacyPathTreatment::new(self.redirect.path_treatment).into_schema(),
        }
    }

    pub fn into_meta_schema(self) -> meta_schema::RedirectRule {
        meta_schema::RedirectRule {
            source: self.redirect.source.as_str().to_owned().into(),
            target: self.redirect.target.as_str().to_owned().into(),
            redirect_status: LegacyRedirectStatus::new(self.redirect.status).into_meta_schema(),
            path_treatment: LegacyPathTreatment::new(self.redirect.path_treatment)
                .into_meta_schema(),
        }
    }
}

struct SchemaMetaRedirect {
    redirect: meta_schema::RedirectRule,
}

impl SchemaMetaRedirect {
    pub fn new(redirect: meta_schema::RedirectRule) -> Self {
        Self { redirect }
    }

    pub fn into_legacy(self) -> RedirectRule {
        RedirectRule {
            source: DomainName::new(self.redirect.source.into_payload()),
            target: signal_domain_criome::UniformResourceLocator::new(
                self.redirect.target.into_payload(),
            ),
            status: MetaSchemaRedirectStatus::new(self.redirect.redirect_status).into_legacy(),
            path_treatment: MetaSchemaPathTreatment::new(self.redirect.path_treatment)
                .into_legacy(),
        }
    }
}

struct LegacyRedirectStatus {
    status: signal_domain_criome::RedirectStatus,
}

impl LegacyRedirectStatus {
    pub fn new(status: signal_domain_criome::RedirectStatus) -> Self {
        Self { status }
    }

    pub fn into_schema(self) -> ordinary_schema::RedirectStatus {
        match self.status {
            signal_domain_criome::RedirectStatus::Permanent => {
                ordinary_schema::RedirectStatus::Permanent
            }
            signal_domain_criome::RedirectStatus::Temporary => {
                ordinary_schema::RedirectStatus::Temporary
            }
        }
    }

    pub fn into_meta_schema(self) -> meta_schema::RedirectStatus {
        match self.status {
            signal_domain_criome::RedirectStatus::Permanent => {
                meta_schema::RedirectStatus::Permanent
            }
            signal_domain_criome::RedirectStatus::Temporary => {
                meta_schema::RedirectStatus::Temporary
            }
        }
    }
}

struct MetaSchemaRedirectStatus {
    status: meta_schema::RedirectStatus,
}

impl MetaSchemaRedirectStatus {
    pub fn new(status: meta_schema::RedirectStatus) -> Self {
        Self { status }
    }

    pub fn into_legacy(self) -> signal_domain_criome::RedirectStatus {
        match self.status {
            meta_schema::RedirectStatus::Permanent => {
                signal_domain_criome::RedirectStatus::Permanent
            }
            meta_schema::RedirectStatus::Temporary => {
                signal_domain_criome::RedirectStatus::Temporary
            }
        }
    }
}

struct LegacyPathTreatment {
    treatment: signal_domain_criome::PathTreatment,
}

impl LegacyPathTreatment {
    pub fn new(treatment: signal_domain_criome::PathTreatment) -> Self {
        Self { treatment }
    }

    pub fn into_schema(self) -> ordinary_schema::PathTreatment {
        match self.treatment {
            signal_domain_criome::PathTreatment::Preserve => {
                ordinary_schema::PathTreatment::Preserve
            }
            signal_domain_criome::PathTreatment::Replace => ordinary_schema::PathTreatment::Replace,
        }
    }

    pub fn into_meta_schema(self) -> meta_schema::PathTreatment {
        match self.treatment {
            signal_domain_criome::PathTreatment::Preserve => meta_schema::PathTreatment::Preserve,
            signal_domain_criome::PathTreatment::Replace => meta_schema::PathTreatment::Replace,
        }
    }
}

struct MetaSchemaPathTreatment {
    treatment: meta_schema::PathTreatment,
}

impl MetaSchemaPathTreatment {
    pub fn new(treatment: meta_schema::PathTreatment) -> Self {
        Self { treatment }
    }

    pub fn into_legacy(self) -> signal_domain_criome::PathTreatment {
        match self.treatment {
            meta_schema::PathTreatment::Preserve => signal_domain_criome::PathTreatment::Preserve,
            meta_schema::PathTreatment::Replace => signal_domain_criome::PathTreatment::Replace,
        }
    }
}

struct LegacyDomainRejection {
    rejection: DomainRequestRejected,
}

impl LegacyDomainRejection {
    pub fn new(rejection: DomainRequestRejected) -> Self {
        Self { rejection }
    }

    pub fn into_schema(self) -> ordinary_schema::RejectedRequest {
        ordinary_schema::RejectedRequest {
            operation: LegacyDomainOperationKind::new(self.rejection.operation).into_schema(),
            reason: LegacyDomainRejectionReason::new(self.rejection.reason).into_schema(),
        }
    }
}

struct LegacyDomainOperationKind {
    operation: signal_domain_criome::OperationKind,
}

impl LegacyDomainOperationKind {
    pub fn new(operation: signal_domain_criome::OperationKind) -> Self {
        Self { operation }
    }

    pub fn into_schema(self) -> ordinary_schema::OperationKind {
        match self.operation {
            signal_domain_criome::OperationKind::Observe => ordinary_schema::OperationKind::Observe,
            signal_domain_criome::OperationKind::Resolve => ordinary_schema::OperationKind::Resolve,
            signal_domain_criome::OperationKind::Project => ordinary_schema::OperationKind::Project,
        }
    }
}

struct LegacyDomainRejectionReason {
    reason: signal_domain_criome::RejectionReason,
}

impl LegacyDomainRejectionReason {
    pub fn new(reason: signal_domain_criome::RejectionReason) -> Self {
        Self { reason }
    }

    pub fn into_schema(self) -> ordinary_schema::RejectionReason {
        match self.reason {
            signal_domain_criome::RejectionReason::DomainUnknown => {
                ordinary_schema::RejectionReason::DomainUnknown
            }
            signal_domain_criome::RejectionReason::DelegationUnknown => {
                ordinary_schema::RejectionReason::DelegationUnknown
            }
            signal_domain_criome::RejectionReason::ProjectionUnavailable => {
                ordinary_schema::RejectionReason::ProjectionUnavailable
            }
        }
    }
}

struct LegacyMetaDelegation {
    delegation: MetaDelegation,
}

impl LegacyMetaDelegation {
    pub fn new(delegation: MetaDelegation) -> Self {
        Self { delegation }
    }

    pub fn into_schema(self) -> meta_schema::Delegation {
        meta_schema::Delegation {
            delegation_name: self.delegation.name.as_str().to_owned().into(),
            domain: self.delegation.domain.as_str().to_owned().into(),
            delegation_target: self.delegation.target.as_str().to_owned().into(),
        }
    }
}

struct SchemaMetaDelegation {
    delegation: meta_schema::Delegation,
}

impl SchemaMetaDelegation {
    pub fn new(delegation: meta_schema::Delegation) -> Self {
        Self { delegation }
    }

    pub fn into_legacy(self) -> MetaDelegation {
        MetaDelegation {
            name: signal_domain_criome::DelegationName::new(
                self.delegation.delegation_name.into_payload(),
            ),
            domain: DomainName::new(self.delegation.domain.into_payload()),
            target: signal_domain_criome::DelegationTarget::new(
                self.delegation.delegation_target.into_payload(),
            ),
        }
    }
}

struct LegacyPolicy {
    policy: meta_signal_domain_criome::Policy,
}

impl LegacyPolicy {
    pub fn new(policy: meta_signal_domain_criome::Policy) -> Self {
        Self { policy }
    }

    pub fn into_schema(self) -> meta_schema::Policy {
        meta_schema::Policy::new(
            self.policy
                .projections
                .into_iter()
                .map(|policy| meta_schema::ProjectionPolicy {
                    domain: policy.domain.as_str().to_owned().into(),
                    projection_scope: LegacyProjectionScope::new(policy.scope).into_meta_schema(),
                    projection_directive: LegacyProjectionDirective::new(policy.directive)
                        .into_schema(),
                })
                .collect(),
        )
    }
}

struct SchemaPolicy {
    policy: meta_schema::Policy,
}

impl SchemaPolicy {
    pub fn new(policy: meta_schema::Policy) -> Self {
        Self { policy }
    }

    pub fn into_legacy(self) -> meta_signal_domain_criome::Policy {
        meta_signal_domain_criome::Policy {
            projections: self
                .policy
                .into_payload()
                .into_iter()
                .map(|policy| ProjectionPolicy {
                    domain: DomainName::new(policy.domain.into_payload()),
                    scope: MetaSchemaProjectionScope::new(policy.projection_scope).into_legacy(),
                    directive: SchemaProjectionDirective::new(policy.projection_directive)
                        .into_legacy(),
                })
                .collect(),
        }
    }
}

struct LegacyProjectionDirective {
    directive: ProjectionDirective,
}

impl LegacyProjectionDirective {
    pub fn new(directive: ProjectionDirective) -> Self {
        Self { directive }
    }

    pub fn into_schema(self) -> meta_schema::ProjectionDirective {
        match self.directive {
            ProjectionDirective::Enable => meta_schema::ProjectionDirective::Enable,
            ProjectionDirective::Disable => meta_schema::ProjectionDirective::Disable,
        }
    }
}

struct SchemaProjectionDirective {
    directive: meta_schema::ProjectionDirective,
}

impl SchemaProjectionDirective {
    pub fn new(directive: meta_schema::ProjectionDirective) -> Self {
        Self { directive }
    }

    pub fn into_legacy(self) -> ProjectionDirective {
        match self.directive {
            meta_schema::ProjectionDirective::Enable => ProjectionDirective::Enable,
            meta_schema::ProjectionDirective::Disable => ProjectionDirective::Disable,
        }
    }
}

struct LegacyProjectionDeclaration {
    declaration: ProjectionDeclaration,
}

impl LegacyProjectionDeclaration {
    pub fn new(declaration: ProjectionDeclaration) -> Self {
        Self { declaration }
    }

    pub fn into_schema(self) -> meta_schema::ProjectionDeclaration {
        meta_schema::ProjectionDeclaration {
            domain: self.declaration.domain.as_str().to_owned().into(),
            records: self
                .declaration
                .records
                .into_iter()
                .map(|record| LegacyRecord::new(record).into_meta_schema())
                .collect(),
            redirects: self
                .declaration
                .redirects
                .into_iter()
                .map(|redirect| LegacyRedirect::new(redirect).into_meta_schema())
                .collect(),
        }
    }
}

struct SchemaProjectionDeclaration {
    declaration: meta_schema::ProjectionDeclaration,
}

impl SchemaProjectionDeclaration {
    pub fn new(declaration: meta_schema::ProjectionDeclaration) -> Self {
        Self { declaration }
    }

    pub fn into_legacy(self) -> ProjectionDeclaration {
        ProjectionDeclaration {
            domain: DomainName::new(self.declaration.domain.into_payload()),
            records: self
                .declaration
                .records
                .into_iter()
                .map(|record| SchemaMetaRecord::new(record).into_legacy())
                .collect(),
            redirects: self
                .declaration
                .redirects
                .into_iter()
                .map(|redirect| SchemaMetaRedirect::new(redirect).into_legacy())
                .collect(),
        }
    }
}

struct LegacyMetaRejection {
    rejection: MetaRequestRejected,
}

impl LegacyMetaRejection {
    pub fn new(rejection: MetaRequestRejected) -> Self {
        Self { rejection }
    }

    pub fn into_schema(self) -> meta_schema::RequestRejected {
        meta_schema::RequestRejected {
            operation: LegacyMetaOperationKind::new(self.rejection.operation).into_schema(),
            reason: LegacyMetaRejectionReason::new(self.rejection.reason).into_schema(),
        }
    }
}

struct LegacyMetaOperationKind {
    operation: meta_signal_domain_criome::OperationKind,
}

impl LegacyMetaOperationKind {
    pub fn new(operation: meta_signal_domain_criome::OperationKind) -> Self {
        Self { operation }
    }

    pub fn into_schema(self) -> meta_schema::OperationKind {
        match self.operation {
            meta_signal_domain_criome::OperationKind::RegisterDomain => {
                meta_schema::OperationKind::RegisterDomain
            }
            meta_signal_domain_criome::OperationKind::Delegate => {
                meta_schema::OperationKind::Delegate
            }
            meta_signal_domain_criome::OperationKind::RetireDomain => {
                meta_schema::OperationKind::RetireDomain
            }
            meta_signal_domain_criome::OperationKind::SetPolicy => {
                meta_schema::OperationKind::SetPolicy
            }
            meta_signal_domain_criome::OperationKind::SetProjection => {
                meta_schema::OperationKind::SetProjection
            }
        }
    }
}

struct LegacyMetaRejectionReason {
    reason: meta_signal_domain_criome::RejectionReason,
}

impl LegacyMetaRejectionReason {
    pub fn new(reason: meta_signal_domain_criome::RejectionReason) -> Self {
        Self { reason }
    }

    pub fn into_schema(self) -> meta_schema::RejectionReason {
        match self.reason {
            meta_signal_domain_criome::RejectionReason::DomainAlreadyRegistered => {
                meta_schema::RejectionReason::DomainAlreadyRegistered
            }
            meta_signal_domain_criome::RejectionReason::DomainUnknown => {
                meta_schema::RejectionReason::DomainUnknown
            }
            meta_signal_domain_criome::RejectionReason::DelegationAlreadyExists => {
                meta_schema::RejectionReason::DelegationAlreadyExists
            }
            meta_signal_domain_criome::RejectionReason::DelegationUnknown => {
                meta_schema::RejectionReason::DelegationUnknown
            }
            meta_signal_domain_criome::RejectionReason::ProjectionUnavailable => {
                meta_schema::RejectionReason::ProjectionUnavailable
            }
        }
    }
}
