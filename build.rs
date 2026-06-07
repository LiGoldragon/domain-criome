use std::{env, path::PathBuf};

use schema_rust_next::{
    MetaListenerTier, NexusDaemonShape, SocketModeBits, WorkingListenerTier,
    build::{DependencySchema, GenerationDriver, GenerationPlan, ModuleEmission},
};

const META_SOCKET_MODE: u32 = 0o600;

fn main() {
    SchemaBuild::from_environment().run();
}

struct SchemaBuild {
    crate_root: PathBuf,
}

impl SchemaBuild {
    fn from_environment() -> Self {
        Self {
            crate_root: PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir set")),
        }
    }

    fn run(&self) {
        println!("cargo:rerun-if-changed=schema/nexus.schema");
        println!("cargo:rerun-if-changed=schema/sema.schema");
        println!("cargo:rerun-if-changed=src/schema/nexus.rs");
        println!("cargo:rerun-if-changed=src/schema/sema.rs");
        println!("cargo:rerun-if-changed=src/schema/daemon.rs");

        let dependencies = ContractSchemaDependencies::from_environment();
        dependencies.emit_rerun_instructions();
        let Some(plan) =
            dependencies.into_generation_plan(&self.crate_root, "domain-criome", "0.1.0")
        else {
            return;
        };

        GenerationDriver::new(plan)
            .generate()
            .expect("generate domain-criome runtime schema artifacts")
            .write_or_check("DOMAIN_CRIOME_UPDATE_SCHEMA_ARTIFACTS")
            .expect("checked-in domain-criome runtime schema artifacts are fresh");
    }
}

struct ContractSchemaDependencies {
    ordinary_signal: Option<DependencySchema>,
    meta_signal: Option<DependencySchema>,
}

impl ContractSchemaDependencies {
    fn from_environment() -> Self {
        Self {
            ordinary_signal: DependencySchema::from_cargo_metadata(
                "signal-domain-criome",
                "signal-domain-criome",
                "0.1.0",
            )
            .expect("read signal-domain-criome schema metadata"),
            meta_signal: DependencySchema::from_cargo_metadata(
                "meta-signal-domain-criome",
                "meta-signal-domain-criome",
                "0.1.0",
            )
            .expect("read meta-signal-domain-criome schema metadata"),
        }
    }

    fn emit_rerun_instructions(&self) {
        println!("cargo:rerun-if-env-changed=DEP_SIGNAL_DOMAIN_CRIOME_SCHEMA_DIR");
        println!("cargo:rerun-if-env-changed=DEP_META_SIGNAL_DOMAIN_CRIOME_SCHEMA_DIR");
    }

    fn into_generation_plan(
        self,
        crate_root: &PathBuf,
        crate_name: &str,
        version: &str,
    ) -> Option<GenerationPlan> {
        match (self.ordinary_signal, self.meta_signal) {
            (Some(ordinary_signal), Some(meta_signal)) => Some(
                GenerationPlan::daemon_runtime(crate_root, crate_name, version)
                    .with_dependency_schema(ordinary_signal)
                    .with_dependency_schema(meta_signal)
                    .with_module(ModuleEmission::daemon_module("nexus", Self::daemon_shape())),
            ),
            (ordinary_signal, meta_signal) => {
                MissingContractSchemas::new(ordinary_signal, meta_signal).warn_and_skip();
                None
            }
        }
    }

    fn daemon_shape() -> NexusDaemonShape {
        NexusDaemonShape::new(
            "domain-criome-daemon",
            WorkingListenerTier::dependency("signal_domain_criome::schema::lib"),
        )
        .with_meta_tier(MetaListenerTier::new(SocketModeBits::new(META_SOCKET_MODE)))
    }
}

struct MissingContractSchemas {
    ordinary_signal: Option<DependencySchema>,
    meta_signal: Option<DependencySchema>,
}

impl MissingContractSchemas {
    fn new(
        ordinary_signal: Option<DependencySchema>,
        meta_signal: Option<DependencySchema>,
    ) -> Self {
        Self {
            ordinary_signal,
            meta_signal,
        }
    }

    fn warn_and_skip(&self) {
        let missing = self.missing_names().join(", ");
        println!(
            "cargo:warning=domain-criome runtime schema generation skipped; missing contract schema metadata for {missing}"
        );
    }

    fn missing_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.ordinary_signal.is_none() {
            names.push("signal-domain-criome");
        }
        if self.meta_signal.is_none() {
            names.push("meta-signal-domain-criome");
        }
        names
    }
}
