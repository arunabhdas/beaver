//! `beaver`: build, run, test, format and package Beaver projects.

mod commands;
mod driver;

use clap::{Parser, Subcommand};
use driver::Failure;
use std::process::ExitCode;

const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (Beaver language 0.1, edition 2026, C11 backend)"
);

#[derive(Parser)]
#[command(
    name = "beaver",
    version = LONG_VERSION,
    about = "The Beaver 0.1 compiler and package manager",
    long_about = "The Beaver 0.1 compiler and package manager.\n\nBeaver source is type-checked and ownership-checked, lowered to C11 and compiled to a native executable with the host C compiler (BEAVER_CC, CC, clang or cc).",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args, Clone, Copy, Default)]
struct DepArgs {
    /// Do not access the network; use only the local package cache
    #[arg(long)]
    offline: bool,
    /// Require beaver.lock to exist and be up to date; never modify it
    #[arg(long)]
    locked: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new package in a new directory
    New {
        /// Package name ([a-z][a-z0-9_]*)
        name: String,
        /// Create a library package instead of a binary
        #[arg(long)]
        lib: bool,
    },
    /// Create a package in the current directory
    Init {
        /// Create a library package instead of a binary
        #[arg(long)]
        lib: bool,
    },
    /// Parse, type-check and ownership-check without generating code
    Check {
        #[command(flatten)]
        deps: DepArgs,
    },
    /// Compile to a native executable under target/{debug,release}
    Build {
        /// Build with optimizations
        #[arg(long)]
        release: bool,
        #[command(flatten)]
        deps: DepArgs,
    },
    /// Build and run the package's executable
    Run {
        /// Build with optimizations
        #[arg(long)]
        release: bool,
        #[command(flatten)]
        deps: DepArgs,
        /// Arguments passed to the program (after `--`)
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Compile and run the `test` blocks in tests/*.bv
    Test {
        /// Build tests with optimizations
        #[arg(long)]
        release: bool,
        #[command(flatten)]
        deps: DepArgs,
    },
    /// Format Beaver sources in src/ and tests/
    Fmt {
        /// Report files that are not formatted instead of rewriting them
        #[arg(long)]
        check: bool,
    },
    /// Add a dependency: NAME@X.Y.Z from the registry, or --path PATH
    Add {
        /// NAME@MAJOR.MINOR.PATCH
        spec: Option<String>,
        /// Add a local library package instead
        #[arg(long, conflicts_with = "spec")]
        path: Option<String>,
        /// Registry base URL (https://..., or loopback http with opt-in)
        #[arg(long)]
        registry_url: Option<String>,
        #[arg(long)]
        offline: bool,
        #[arg(long, hide = true)]
        locked: bool,
    },
    /// Remove a direct dependency
    Remove {
        name: String,
        #[arg(long)]
        offline: bool,
        #[arg(long, hide = true)]
        locked: bool,
    },
    /// Change a registry dependency to an exact version
    Update {
        /// NAME@MAJOR.MINOR.PATCH
        spec: String,
        #[arg(long)]
        registry_url: Option<String>,
        #[arg(long)]
        offline: bool,
        #[arg(long, hide = true)]
        locked: bool,
    },
    /// Resolve and download all dependencies without compiling
    Fetch {
        #[command(flatten)]
        deps: DepArgs,
        #[arg(long)]
        registry_url: Option<String>,
    },
    /// Create a deterministic .tar.gz archive of a library package
    Package {
        /// Output directory (default: target/package)
        #[arg(long)]
        output: Option<String>,
        #[command(flatten)]
        deps: DepArgs,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::New { name, lib } => commands::new(&name, lib),
        Command::Init { lib } => commands::init(lib),
        Command::Check { deps } => commands::check(deps.into()),
        Command::Build { release, deps } => commands::build(release, deps.into()),
        Command::Run {
            release,
            deps,
            args,
        } => commands::run(release, deps.into(), &args),
        Command::Test { release, deps } => commands::test(release, deps.into()),
        Command::Fmt { check } => commands::fmt(check),
        Command::Add {
            spec,
            path,
            registry_url,
            offline,
            locked,
        } => commands::add(
            spec.as_deref(),
            path.as_deref(),
            registry_url.as_deref(),
            offline,
            locked,
        ),
        Command::Remove {
            name,
            offline,
            locked,
        } => commands::remove(&name, offline, locked),
        Command::Update {
            spec,
            registry_url,
            offline,
            locked,
        } => commands::update(&spec, registry_url.as_deref(), offline, locked),
        Command::Fetch { deps, registry_url } => commands::fetch(driver::DepFlags {
            offline: deps.offline,
            locked: deps.locked,
            registry_url: registry_url.as_deref(),
        }),
        Command::Package { output, deps } => commands::package(output.as_deref(), deps.into()),
    };
    match result {
        Ok(code) => code,
        Err(Failure::Diag(d)) => {
            eprint!("{}", d.render(None));
            ExitCode::from(1)
        }
        Err(Failure::Rendered(text)) => {
            eprint!("{text}");
            ExitCode::from(1)
        }
    }
}

impl From<DepArgs> for driver::DepFlags<'static> {
    fn from(d: DepArgs) -> Self {
        driver::DepFlags {
            offline: d.offline,
            locked: d.locked,
            registry_url: None,
        }
    }
}
