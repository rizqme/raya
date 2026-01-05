//! Raya unified CLI tool
//!
//! Single command-line interface for all Raya operations:
//! compilation, execution, testing, package management, and more.
//!
//! See design/CLI.md for complete specification.

use clap::{Parser, Subcommand};

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

    /// Build/compile Raya source to binary (.rbin)
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

    /// Install dependencies
    Install {
        /// Package to install (if not specified, installs all from raya.toml)
        package: Option<String>,
        /// Save to dependencies
        #[arg(short = 'S', long)]
        save: bool,
        /// Save to devDependencies
        #[arg(short = 'D', long)]
        save_dev: bool,
        /// Install globally
        #[arg(short, long)]
        global: bool,
    },

    /// Add a dependency (alias for install <package>)
    Add {
        /// Package to add
        package: String,
        /// Save to devDependencies
        #[arg(short = 'D', long)]
        dev: bool,
    },

    /// Remove a dependency
    Remove {
        /// Package to remove
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

    /// Initialize a new project
    Init {
        /// Project name
        name: Option<String>,
        /// Project template
        #[arg(short, long)]
        template: Option<String>,
        /// Skip prompts
        #[arg(short, long)]
        yes: bool,
    },

    /// Create from template (alias for init)
    Create {
        /// Project name
        name: String,
        /// Project template
        #[arg(short, long)]
        template: Option<String>,
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
            println!("Build type: binary (.rbin with mandatory reflection)");
            if release {
                println!("Release mode: enabled");
            }
            if watch {
                println!("Watch mode: enabled");
            }
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
            package,
            save,
            save_dev,
            global,
        } => {
            if let Some(pkg) = package {
                println!("Installing package: {}", pkg);
                if save {
                    println!("Saving to dependencies");
                }
                if save_dev {
                    println!("Saving to devDependencies");
                }
            } else {
                println!("Installing all dependencies...");
            }
            if global {
                println!("Global installation");
            }
            println!("(Not yet implemented)");
        }

        Commands::Add { package, dev } => {
            println!("Adding package: {}", package);
            if dev {
                println!("Adding to devDependencies");
            }
            println!("(Not yet implemented)");
        }

        Commands::Remove { package } => {
            println!("Removing package: {}", package);
            println!("(Not yet implemented)");
        }

        Commands::Update { package } => {
            if let Some(pkg) = package {
                println!("Updating package: {}", pkg);
            } else {
                println!("Updating all dependencies...");
            }
            println!("(Not yet implemented)");
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

        Commands::Init {
            name,
            template,
            yes,
        } => {
            if let Some(n) = name {
                println!("Initializing project: {}", n);
            } else {
                println!("Initializing project in current directory...");
            }
            if let Some(t) = template {
                println!("Using template: {}", t);
            }
            if yes {
                println!("Skipping prompts");
            }
            println!("(Not yet implemented)");
        }

        Commands::Create { name, template } => {
            println!("Creating project: {}", name);
            if let Some(t) = template {
                println!("Using template: {}", t);
            }
            println!("(Not yet implemented)");
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
