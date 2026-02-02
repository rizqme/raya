//! Raya unified CLI tool
//!
//! Single command-line interface for all Raya operations:
//! compilation, execution, testing, package management, and more.
//!
//! See design/CLI.md for complete specification.

use clap::{Parser, Subcommand};
use rpkg::commands::{add, init, install};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "raya")]
#[command(about = "Raya programming language toolchain", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a Raya file
    Run {
        /// Input file
        file: String,
        /// Arguments to pass to the program
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
        /// Watch for changes and reload
        #[arg(short, long)]
        watch: bool,
        /// Enable debugger
        #[arg(long)]
        inspect: bool,
    },

    /// Build/compile Raya source to binary (.ryb)
    Build {
        /// Files or directories to build
        #[arg(default_value = ".")]
        files: Vec<String>,
        /// Output directory
        #[arg(short, long, default_value = "dist")]
        out_dir: String,
        /// Optimized release build
        #[arg(short, long)]
        release: bool,
        /// Watch for changes
        #[arg(short, long)]
        watch: bool,
    },

    /// Type-check without building
    Check {
        /// Files or directories to check
        #[arg(default_value = ".")]
        files: Vec<String>,
        /// Watch for changes
        #[arg(short, long)]
        watch: bool,
    },

    /// Run tests
    Test {
        /// Test name pattern to match
        pattern: Option<String>,
        /// Watch for changes
        #[arg(short, long)]
        watch: bool,
        /// Generate coverage report
        #[arg(long)]
        coverage: bool,
        /// Stop after first failure
        #[arg(long)]
        bail: bool,
    },

    /// Install dependencies from raya.toml
    Install {
        /// Only install production dependencies
        #[arg(long)]
        production: bool,
        /// Force re-download even if cached
        #[arg(short, long)]
        force: bool,
        /// Update to latest compatible versions
        #[arg(short, long)]
        update: bool,
    },

    /// Add a dependency
    Add {
        /// Package specifier (e.g., "logging", "logging@1.2.0", "logging@^1.0.0")
        package: String,
        /// Add as dev dependency
        #[arg(short = 'D', long)]
        dev: bool,
        /// Use exact version (no caret prefix)
        #[arg(short, long)]
        exact: bool,
        /// Don't install after adding
        #[arg(long)]
        no_install: bool,
    },

    /// Remove a dependency
    Remove {
        /// Package name to remove
        package: String,
    },

    /// Update dependencies
    Update {
        /// Package to update (if not specified, updates all)
        package: Option<String>,
    },

    /// Publish package to registry
    Publish {
        /// Publish tag
        #[arg(long, default_value = "latest")]
        tag: String,
        /// Dry run (don't actually publish)
        #[arg(long)]
        dry_run: bool,
    },

    /// Format code
    Fmt {
        /// Files or directories to format
        #[arg(default_value = ".")]
        files: Vec<String>,
        /// Check if files are formatted
        #[arg(long)]
        check: bool,
    },

    /// Lint code
    Lint {
        /// Files or directories to lint
        #[arg(default_value = ".")]
        files: Vec<String>,
        /// Auto-fix issues
        #[arg(long)]
        fix: bool,
    },

    /// Generate documentation
    Doc {
        /// Output directory
        #[arg(short, long, default_value = "docs")]
        out_dir: String,
        /// Start documentation server
        #[arg(long)]
        serve: bool,
        /// Open in browser
        #[arg(long)]
        open: bool,
    },

    /// Start interactive REPL
    Repl,

    /// Run benchmarks
    Bench {
        /// Benchmark name pattern to match
        pattern: Option<String>,
    },

    /// Create standalone executable with embedded runtime
    Bundle {
        /// Input file
        file: String,
        /// Output file path
        #[arg(short, long)]
        output: String,
        /// Target platform
        #[arg(long, default_value = "native")]
        target: String,
        /// Optimized release build
        #[arg(short, long)]
        release: bool,
        /// Strip debug symbols
        #[arg(long)]
        strip: bool,
        /// Compress executable
        #[arg(long)]
        compress: bool,
        /// Don't embed runtime
        #[arg(long)]
        no_runtime: bool,
    },

    /// Initialize a new Raya project
    Init {
        /// Directory to initialize (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Project name (defaults to directory name)
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Create a new project (alias for init)
    New {
        /// Project name
        name: String,
    },

    /// Upgrade Raya installation
    Upgrade {
        /// Version to upgrade to
        version: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            file,
            args,
            watch,
            inspect,
        } => {
            if watch {
                println!("Running {} in watch mode...", file);
            } else if inspect {
                println!("Running {} with debugger...", file);
            } else {
                println!("Running: {}", file);
            }
            if !args.is_empty() {
                println!("Arguments: {:?}", args);
            }
            println!("(Not yet implemented)");
        }

        Commands::Build {
            files,
            out_dir,
            release,
            watch,
        } => {
            println!("Building: {:?}", files);
            println!("Output directory: {}", out_dir);
            if release {
                println!("Release mode: enabled");
            }
            if watch {
                println!("Watch mode: enabled");
            }
            // Reflection metadata is always emitted
            println!("(Not yet implemented)");
        }

        Commands::Check { files, watch } => {
            println!("Type-checking: {:?}", files);
            if watch {
                println!("Watch mode: enabled");
            }
            println!("(Not yet implemented)");
        }

        Commands::Test {
            pattern,
            watch,
            coverage,
            bail,
        } => {
            if let Some(p) = pattern {
                println!("Running tests matching: {}", p);
            } else {
                println!("Running all tests...");
            }
            if watch {
                println!("Watch mode: enabled");
            }
            if coverage {
                println!("Coverage: enabled");
            }
            if bail {
                println!("Bail on first failure: enabled");
            }
            println!("(Not yet implemented)");
        }

        Commands::Install {
            production,
            force,
            update,
        } => {
            let options = install::InstallOptions {
                production,
                force,
                update,
            };
            match install::install_dependencies(None, options) {
                Ok(result) => {
                    println!(
                        "\nDone! {} installed, {} from cache, {} updated.",
                        result.installed, result.cached, result.updated
                    );
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Add {
            package,
            dev,
            exact,
            no_install,
        } => {
            let options = add::AddOptions {
                dev,
                exact,
                no_install,
            };
            match add::add_package(&package, None, options) {
                Ok(()) => {
                    println!("\nPackage added successfully.");
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Remove { package } => {
            match add::remove_package(&package, None) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Update { package } => {
            // Update is essentially install with --update flag
            let options = install::InstallOptions {
                production: false,
                force: false,
                update: true,
            };
            if let Some(pkg) = package {
                println!("Updating package: {}", pkg);
                println!("(Single package update not yet implemented)");
            } else {
                match install::install_dependencies(None, options) {
                    Ok(result) => {
                        println!(
                            "\nDone! {} installed, {} from cache, {} updated.",
                            result.installed, result.cached, result.updated
                        );
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }

        Commands::Publish { tag, dry_run } => {
            println!("Publishing package...");
            println!("Tag: {}", tag);
            if dry_run {
                println!("(Dry run - not actually publishing)");
            }
            println!("(Not yet implemented)");
        }

        Commands::Fmt { files, check } => {
            println!("Formatting: {:?}", files);
            if check {
                println!("Check mode: enabled");
            }
            println!("(Not yet implemented)");
        }

        Commands::Lint { files, fix } => {
            println!("Linting: {:?}", files);
            if fix {
                println!("Auto-fix: enabled");
            }
            println!("(Not yet implemented)");
        }

        Commands::Doc {
            out_dir,
            serve,
            open,
        } => {
            println!("Generating documentation...");
            println!("Output directory: {}", out_dir);
            if serve {
                println!("Starting documentation server...");
            }
            if open {
                println!("Opening in browser...");
            }
            println!("(Not yet implemented)");
        }

        Commands::Repl => {
            println!("Starting REPL...");
            println!("(Not yet implemented)");
        }

        Commands::Bench { pattern } => {
            if let Some(p) = pattern {
                println!("Running benchmarks matching: {}", p);
            } else {
                println!("Running all benchmarks...");
            }
            println!("(Not yet implemented)");
        }

        Commands::Bundle {
            file,
            output,
            target,
            release,
            strip,
            compress,
            no_runtime,
        } => {
            println!("Bundling: {}", file);
            println!("Output: {}", output);
            println!("Target: {}", target);
            if release {
                println!("Release mode: enabled");
            }
            if strip {
                println!("Strip symbols: enabled");
            }
            if compress {
                println!("Compression: enabled");
            }
            if no_runtime {
                println!("Embedded runtime: disabled");
            } else {
                println!("Embedded runtime: enabled");
            }
            println!("(Not yet implemented)");
        }

        Commands::Init { path, name } => {
            match init::init_project(&path, name.as_deref()) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::New { name } => {
            let path = PathBuf::from(&name);
            match init::init_project(&path, Some(&name)) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Upgrade { version } => {
            if let Some(v) = version {
                println!("Upgrading to version: {}", v);
            } else {
                println!("Upgrading to latest version...");
            }
            println!("(Not yet implemented)");
        }
    }

    Ok(())
}
