use domain_criome::Store;
use owner_signal_domain_criome::{
    Delegation as OwnerDelegation, DelegationTarget, Operation as OwnerOperation, Policy,
    ProjectionDirective, ProjectionPolicy, Registration, Reply as OwnerReply,
};
use signal_domain_criome::{
    DelegationName, DomainName, Observation, Operation as DomainOperation, ProjectionQuery,
    ProjectionScope, RecordKind, Reply as DomainReply, ResolutionQuery, ResolutionScope,
};
use signal_frame::{Reply as FrameReply, RequestPayload, SubReply};

#[test]
fn owner_operations_update_in_memory_domain_state() {
    let store = Store::new();

    assert!(matches!(
        owner_reply(
            &store,
            OwnerOperation::RegisterDomain(Registration {
                domain: DomainName::new("goldragon.criome"),
            })
        ),
        OwnerReply::DomainRegistered(_)
    ));
    assert!(matches!(
        owner_reply(
            &store,
            OwnerOperation::Delegate(OwnerDelegation {
                name: DelegationName::new("www"),
                domain: DomainName::new("goldragon.criome"),
                target: DelegationTarget::new("203.0.113.10"),
            })
        ),
        OwnerReply::DomainDelegated(_)
    ));

    let domains = domain_reply(
        &store,
        DomainOperation::Observe(Observation::Domains(signal_domain_criome::DomainQuery {
            root: None,
        })),
    );
    let DomainReply::Observed(signal_domain_criome::ObservationResult::Domains(listing)) = domains
    else {
        panic!("expected domain listing");
    };
    assert_eq!(listing.domains, vec![DomainName::new("goldragon.criome")]);

    let delegations = domain_reply(
        &store,
        DomainOperation::Observe(Observation::Delegations(
            signal_domain_criome::DelegationQuery {
                domain: Some(DomainName::new("goldragon.criome")),
            },
        )),
    );
    let DomainReply::Observed(signal_domain_criome::ObservationResult::Delegations(listing)) =
        delegations
    else {
        panic!("expected delegation listing");
    };
    assert_eq!(listing.delegations.len(), 1);
    assert_eq!(listing.delegations[0].name, DelegationName::new("www"));
}

#[test]
fn public_record_projection_and_resolution_use_delegation_state() {
    let store = registered_store();

    let projection = domain_reply(
        &store,
        DomainOperation::Project(ProjectionQuery {
            domain: DomainName::new("goldragon.criome"),
            scope: ProjectionScope::PublicRecords,
        }),
    );
    let DomainReply::Projected(projection) = projection else {
        panic!("expected projection");
    };
    assert_eq!(projection.records.len(), 1);
    assert_eq!(
        projection.records[0].name,
        DomainName::new("www.goldragon.criome")
    );
    assert_eq!(projection.records[0].kind, RecordKind::AddressV4);
    assert_eq!(projection.records[0].value.as_str(), "203.0.113.10");
    assert!(projection.redirects.is_empty());

    let resolution = domain_reply(
        &store,
        DomainOperation::Resolve(ResolutionQuery {
            name: DomainName::new("www.goldragon.criome"),
            scope: ResolutionScope::Public,
        }),
    );
    let DomainReply::Resolved(resolution) = resolution else {
        panic!("expected resolution");
    };
    assert_eq!(resolution.addresses.len(), 1);
    assert_eq!(resolution.addresses[0].address.as_str(), "203.0.113.10");
}

#[test]
fn unknown_domains_and_unimplemented_redirect_projection_are_typed_rejections() {
    let store = registered_store();

    let unknown = domain_reply(
        &store,
        DomainOperation::Project(ProjectionQuery {
            domain: DomainName::new("missing.criome"),
            scope: ProjectionScope::PublicRecords,
        }),
    );
    assert!(matches!(
        unknown,
        DomainReply::RequestRejected(signal_domain_criome::RequestRejected {
            operation: signal_domain_criome::OperationKind::Project,
            reason: signal_domain_criome::RejectionReason::DomainUnknown,
        })
    ));

    let redirect_projection = domain_reply(
        &store,
        DomainOperation::Project(ProjectionQuery {
            domain: DomainName::new("goldragon.criome"),
            scope: ProjectionScope::RedirectRules,
        }),
    );
    assert!(matches!(
        redirect_projection,
        DomainReply::RequestRejected(signal_domain_criome::RequestRejected {
            operation: signal_domain_criome::OperationKind::Project,
            reason: signal_domain_criome::RejectionReason::ProjectionUnavailable,
        })
    ));
}

#[test]
fn owner_rejections_are_typed() {
    let store = Store::new();

    let delegation = owner_reply(
        &store,
        OwnerOperation::Delegate(OwnerDelegation {
            name: DelegationName::new("www"),
            domain: DomainName::new("missing.criome"),
            target: DelegationTarget::new("203.0.113.10"),
        }),
    );
    assert!(matches!(
        delegation,
        OwnerReply::RequestRejected(owner_signal_domain_criome::RequestRejected {
            operation: owner_signal_domain_criome::OperationKind::Delegate,
            reason: owner_signal_domain_criome::RejectionReason::DomainUnknown,
        })
    ));

    owner_reply(
        &store,
        OwnerOperation::RegisterDomain(Registration {
            domain: DomainName::new("goldragon.criome"),
        }),
    );
    let duplicate = owner_reply(
        &store,
        OwnerOperation::RegisterDomain(Registration {
            domain: DomainName::new("goldragon.criome"),
        }),
    );
    assert!(matches!(
        duplicate,
        OwnerReply::RequestRejected(owner_signal_domain_criome::RequestRejected {
            operation: owner_signal_domain_criome::OperationKind::RegisterDomain,
            reason: owner_signal_domain_criome::RejectionReason::DomainAlreadyRegistered,
        })
    ));
}

#[test]
fn projection_policy_can_disable_public_records() {
    let store = registered_store();
    let policy = owner_reply(
        &store,
        OwnerOperation::SetPolicy(Policy {
            projections: vec![ProjectionPolicy {
                domain: DomainName::new("goldragon.criome"),
                scope: ProjectionScope::PublicRecords,
                directive: ProjectionDirective::Disable,
            }],
        }),
    );
    assert!(matches!(
        policy,
        OwnerReply::PolicySet(owner_signal_domain_criome::PolicySet {
            projection_policy_count: 1,
        })
    ));

    let projection = domain_reply(
        &store,
        DomainOperation::Project(ProjectionQuery {
            domain: DomainName::new("goldragon.criome"),
            scope: ProjectionScope::PublicRecords,
        }),
    );
    assert!(matches!(
        projection,
        DomainReply::RequestRejected(signal_domain_criome::RequestRejected {
            operation: signal_domain_criome::OperationKind::Project,
            reason: signal_domain_criome::RejectionReason::ProjectionUnavailable,
        })
    ));
}

fn registered_store() -> Store {
    let store = Store::new();
    owner_reply(
        &store,
        OwnerOperation::RegisterDomain(Registration {
            domain: DomainName::new("goldragon.criome"),
        }),
    );
    owner_reply(
        &store,
        OwnerOperation::Delegate(OwnerDelegation {
            name: DelegationName::new("www"),
            domain: DomainName::new("goldragon.criome"),
            target: DelegationTarget::new("203.0.113.10"),
        }),
    );
    store
}

fn domain_reply(store: &Store, operation: DomainOperation) -> DomainReply {
    match store.handle_ordinary_request(operation.into_request()) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail() {
            (SubReply::Ok(reply), tail) if tail.is_empty() => reply,
            other => panic!("unexpected subreply: {other:?}"),
        },
        other => panic!("unexpected reply: {other:?}"),
    }
}

fn owner_reply(store: &Store, operation: OwnerOperation) -> OwnerReply {
    match store.handle_owner_request(operation.into_request()) {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail() {
            (SubReply::Ok(reply), tail) if tail.is_empty() => reply,
            other => panic!("unexpected subreply: {other:?}"),
        },
        other => panic!("unexpected reply: {other:?}"),
    }
}
