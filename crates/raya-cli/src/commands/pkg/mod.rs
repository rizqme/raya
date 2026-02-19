//! `raya pkg` — Registry and authentication management.

use clap::Subcommand;

mod info;
mod login;
mod logout;
mod set_url;
mod whoami;

#[derive(Subcommand)]
pub enum PkgCommands {
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
