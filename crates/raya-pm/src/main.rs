//! Raya Package Manager (rayapm)

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rayapm")]
#[command(about = "Raya package manager", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Raya project
    Init,
    /// Install dependencies
    Install,
    /// Add a dependency
    Add {
        /// Package name
        package: String,
    },
    /// Remove a dependency
    Remove {
        /// Package name
        package: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            println!("Initializing new Raya project...");
            println!("(Not yet implemented)");
        }
        Commands::Install => {
            println!("Installing dependencies...");
            println!("(Not yet implemented)");
        }
        Commands::Add { package } => {
            println!("Adding package: {}", package);
            println!("(Not yet implemented)");
        }
        Commands::Remove { package } => {
            println!("Removing package: {}", package);
            println!("(Not yet implemented)");
        }
    }

    Ok(())
}
