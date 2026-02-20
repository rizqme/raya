//! `raya pkg` — Package management commands.
//!
//! This is the canonical home for all package management operations.
//! Common commands (init, install, add, remove, update, publish, upgrade)
//! are also available as top-level aliases for convenience.

use clap::Subcommand;
use std::path::PathBuf;

mod info;
mod login;
mod logout;
mod set_url;
mod whoami;

#[derive(Subcommand)]
pub enum PkgCommands {
    /// Initialize a new Raya project
    Init {
        /// Directory to initialize
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Project name
        #[arg(short, long)]
        name: Option<String>,
        /// Project template
        #[arg(long, default_value = "basic")]
        template: String,
        /// Skip interactive prompts
        #[arg(short, long)]
        yes: bool,
    },

    /// Install all dependencies
    #[command(alias = "i")]
    Install {
        /// Only install production dependencies
        #[arg(long)]
        production: bool,
        /// Error if lockfile would change (for CI)
        #[arg(long)]
        frozen: bool,
        /// Force re-download even if cached
        #[arg(short, long)]
        force: bool,
    },

    /// Add a dependency
    #[command(alias = "a")]
    Add {
        /// Package specifier (e.g., "logging", "logging@1.2.0")
        package: String,
        /// Add as dev dependency
        #[arg(short = 'D', long)]
        dev: bool,
        /// Use exact version (no ^ prefix)
        #[arg(short = 'E', long)]
        exact: bool,
        /// Don't install after adding
        #[arg(long)]
        no_install: bool,
    },

    /// Remove a dependency
    #[command(alias = "rm")]
    Remove {
        /// Package name to remove
        package: String,
    },

    /// Update dependencies
    Update {
        /// Package to update (all if not specified)
        package: Option<String>,
    },

    /// Publish package to registry
    Publish {
        /// Publish tag
        #[arg(long, default_value = "latest")]
        tag: String,
        /// Don't actually publish
        #[arg(long)]
        dry_run: bool,
        /// Access level
        #[arg(long, default_value = "public")]
        access: String,
    },

    /// Upgrade Raya installation
    Upgrade {
        /// Version to upgrade to
        version: Option<String>,
        /// Check for updates without installing
        #[arg(long)]
        check: bool,
        /// Force reinstall
        #[arg(long)]
        force: bool,
    },

    /// Authenticate with a package registry
    Login {
        /// Registry URL to authenticate with
        #[arg(long)]
        registry: Option<String>,
        /// Provide token directly (skip interactive prompt)
        #[arg(long)]
        token: Option<String>,
        /// Associate credentials with a scope (@org)
        #[arg(long)]
        scope: Option<String>,
    },

    /// Remove stored credentials for a registry
    Logout {
        /// Registry to log out of
        #[arg(long)]
        registry: Option<String>,
        /// Log out of specific scope only
        #[arg(long)]
        scope: Option<String>,
    },

    /// Set the default package registry URL
    SetUrl {
        /// Registry URL
        url: Option<String>,
        /// Set globally in ~/.raya/config.toml
        #[arg(long)]
        global: bool,
        /// Show current registry URL
        #[arg(long)]
        show: bool,
    },

    /// Show the currently authenticated user
    Whoami,

    /// Show metadata for a package from the registry
    Info {
        /// Package name
        package: String,
    },
}

pub fn execute(cmd: PkgCommands) -> anyhow::Result<()> {
    match cmd {
        PkgCommands::Init { path, name, template, yes } =>
            super::init::execute(path, name, template, yes),
        PkgCommands::Install { production, frozen, force } =>
            super::install::execute(production, frozen, force),
        PkgCommands::Add { package, dev, exact, no_install } =>
            super::add::execute(package, dev, exact, no_install),
        PkgCommands::Remove { package } =>
            super::remove::execute(package),
        PkgCommands::Update { package } =>
            super::update::execute(package),
        PkgCommands::Publish { tag, dry_run, access } =>
            super::publish::execute(tag, dry_run, access),
        PkgCommands::Upgrade { version, check, force } =>
            super::upgrade::execute(version, check, force),
        PkgCommands::Login { registry, token, scope } =>
            login::execute(registry, token, scope),
        PkgCommands::Logout { registry, scope } =>
            logout::execute(registry, scope),
        PkgCommands::SetUrl { url, global, show } =>
            set_url::execute(url, global, show),
        PkgCommands::Whoami =>
            whoami::execute(),
        PkgCommands::Info { package } =>
            info::execute(package),
    }
}
