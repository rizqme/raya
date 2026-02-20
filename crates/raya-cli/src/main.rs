//! Raya unified CLI tool
//!
//! Single command-line interface for all Raya operations:
//! compilation, execution, testing, package management, and more.

mod commands;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "raya")]
#[command(about = "Raya programming language toolchain")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a script or execute a file
    #[command(alias = "r")]
    Run {
        /// Script name (from [scripts] in raya.toml) or file path
        target: Option<String>,
        /// Arguments to pass to the program
        #[arg(trailing_var_arg = true, last = true)]
        args: Vec<String>,
        /// Watch for changes and re-run
        #[arg(short, long)]
        watch: bool,
        /// Enable debugger
        #[arg(long)]
        inspect: bool,
        /// Enable debugger and break at entry
        #[arg(long)]
        inspect_brk: bool,
        /// Skip bytecode cache, always recompile
        #[arg(long)]
        no_cache: bool,
        /// Disable JIT compilation (interpreter only)
        #[arg(long)]
        no_jit: bool,
        /// JIT adaptive compilation call threshold
        #[arg(long, default_value = "1000")]
        jit_threshold: u32,
        /// Worker thread count (0 = auto)
        #[arg(long, default_value = "0")]
        threads: usize,
        /// Max heap size in MB
        #[arg(long, default_value = "512")]
        heap_limit: usize,
        /// Maximum execution time in ms (0 = unlimited)
        #[arg(long, default_value = "0")]
        timeout: u64,
        /// List available scripts from raya.toml
        #[arg(long)]
        list: bool,
    },

    /// Compile to bytecode (.ryb)
    #[command(alias = "b")]
    Build {
        /// Files or directories to build
        #[arg(default_value = ".")]
        files: Vec<String>,
        /// Output directory
        #[arg(short, long, default_value = "dist")]
        out_dir: String,
        /// Enable all optimizations
        #[arg(short, long)]
        release: bool,
        /// Watch for changes
        #[arg(short, long)]
        watch: bool,
        /// Emit debug source mapping
        #[arg(long)]
        sourcemap: bool,
        /// Show what would be built without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Type-check without building
    #[command(alias = "c")]
    Check {
        /// Files or directories to check
        #[arg(default_value = ".")]
        files: Vec<String>,
        /// Watch for changes
        #[arg(short, long)]
        watch: bool,
        /// Treat all warnings as errors
        #[arg(long)]
        strict: bool,
        /// Diagnostic format (pretty, json)
        #[arg(long, default_value = "pretty")]
        format: String,
        /// Suppress specific warnings (e.g., --allow unused-variable)
        #[arg(long = "allow", value_name = "WARNING")]
        allow: Vec<String>,
        /// Treat specific warnings as errors (e.g., --deny shadowed-variable)
        #[arg(long = "deny", value_name = "WARNING")]
        deny: Vec<String>,
        /// Suppress all warnings
        #[arg(long)]
        no_warnings: bool,
    },

    /// Evaluate an inline expression
    Eval {
        /// Code to evaluate
        code: String,
        /// Print the result of the last expression
        #[arg(long)]
        print: bool,
        /// Don't auto-print the result
        #[arg(long)]
        no_print: bool,
        /// Disable JIT for the evaluated code
        #[arg(long)]
        no_jit: bool,
    },

    /// Run tests
    #[command(alias = "t")]
    Test {
        /// Test name pattern to match
        filter: Option<String>,
        /// Watch for changes
        #[arg(short, long)]
        watch: bool,
        /// Generate coverage report
        #[arg(long)]
        coverage: bool,
        /// Stop after first failure
        #[arg(long)]
        bail: bool,
        /// Per-test timeout in ms
        #[arg(long, default_value = "5000")]
        timeout: u64,
        /// Max parallel test files
        #[arg(long, default_value = "0")]
        concurrency: usize,
        /// Output format
        #[arg(long, default_value = "default")]
        reporter: String,
        /// Filter test files by glob
        #[arg(long)]
        file: Option<String>,
        /// Update snapshot expectations
        #[arg(long)]
        update_snapshots: bool,
    },

    /// Run benchmarks
    Bench {
        /// Benchmark name pattern to match
        filter: Option<String>,
        /// Warmup iterations
        #[arg(long, default_value = "100")]
        warmup: usize,
        /// Benchmark iterations
        #[arg(long, default_value = "1000")]
        iterations: usize,
        /// Save results to JSON file
        #[arg(long)]
        save: Option<String>,
        /// Compare against saved results
        #[arg(long)]
        compare: Option<String>,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },

    /// Format source files
    Fmt {
        /// Files or directories to format
        #[arg(default_value = ".")]
        files: Vec<String>,
        /// Check formatting without writing
        #[arg(long)]
        check: bool,
        /// Show diff of formatting changes
        #[arg(long)]
        diff: bool,
        /// Read from stdin, write to stdout
        #[arg(long)]
        stdin: bool,
    },

    /// Lint source files
    Lint {
        /// Files or directories to lint
        #[arg(default_value = ".")]
        files: Vec<String>,
        /// Auto-fix issues where possible
        #[arg(long)]
        fix: bool,
        /// Output format
        #[arg(long, default_value = "pretty")]
        format: String,
        /// Watch mode
        #[arg(short, long)]
        watch: bool,
    },

    /// Start interactive REPL
    Repl {
        /// Disable JIT in REPL
        #[arg(long)]
        no_jit: bool,
    },

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

    /// Package management (init, install, add, remove, login, ...)
    Pkg {
        #[command(subcommand)]
        command: commands::pkg::PkgCommands,
    },

    /// Create standalone executable
    Bundle {
        /// Input file
        file: String,
        /// Output file path
        #[arg(short, long)]
        output: String,
        /// Target platform
        #[arg(long, default_value = "native")]
        target: String,
        /// Full optimizations
        #[arg(short, long)]
        release: bool,
        /// Strip debug info
        #[arg(long)]
        strip: bool,
        /// Compress with LZ4
        #[arg(long)]
        compress: bool,
        /// Don't embed VM
        #[arg(long)]
        no_runtime: bool,
    },

    /// Generate documentation
    Doc {
        /// Output directory
        #[arg(short, long, default_value = "docs/api")]
        out_dir: String,
        /// Start documentation server
        #[arg(long)]
        serve: bool,
        /// Open in browser
        #[arg(long)]
        open: bool,
        /// Output format
        #[arg(long, default_value = "html")]
        format: String,
    },

    /// Start Language Server
    Lsp {
        /// Communication via stdin/stdout (default)
        #[arg(long)]
        stdio: bool,
        /// TCP port
        #[arg(long)]
        port: Option<u16>,
    },

    /// Generate shell completions
    Completions {
        /// Shell type (bash, zsh, fish, powershell)
        shell: String,
    },

    /// Clear caches and build artifacts
    Clean {
        /// Only clear bytecode cache
        #[arg(long)]
        cache: bool,
        /// Only clear build output
        #[arg(long)]
        dist: bool,
        /// Clear everything
        #[arg(long)]
        all: bool,
    },

    /// Display environment and project info
    Info,

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
}

/// Check if a string looks like a Raya file path
fn looks_like_raya_file(arg: &str) -> bool {
    arg.ends_with(".raya") || arg.ends_with(".ryb")
}

fn main() -> anyhow::Result<()> {
    // Handle implicit run: `raya ./file.raya` or `raya src/main.raya`
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.len() > 1 && looks_like_raya_file(&raw_args[1]) {
        return commands::run::execute_file(&raw_args[1], &raw_args[2..]);
    }

    let cli = Cli::parse();

    match cli.command {
        Some(cmd) => dispatch(cmd),
        None => {
            use clap::CommandFactory;
            Cli::command().print_help()?;
            Ok(())
        }
    }
}

fn dispatch(cmd: Commands) -> anyhow::Result<()> {
    match cmd {
        Commands::Run {
            target, args, watch, inspect, inspect_brk,
            no_cache, no_jit, jit_threshold, threads,
            heap_limit, timeout, list,
        } => commands::run::execute(commands::run::RunArgs {
            target, args, watch, inspect, inspect_brk,
            no_cache, no_jit, jit_threshold, threads,
            heap_limit, timeout, list,
        }),

        Commands::Build { files, out_dir, release, watch, sourcemap, dry_run } =>
            commands::build::execute(files, out_dir, release, watch, sourcemap, dry_run),

        Commands::Check { files, watch, strict, format, allow, deny, no_warnings } =>
            commands::check::execute(files, watch, strict, format, allow, deny, no_warnings),

        Commands::Eval { code, print, no_print, no_jit } =>
            commands::eval::execute(code, print, no_print, no_jit),

        Commands::Test {
            filter, watch, coverage, bail, timeout,
            concurrency, reporter, file, update_snapshots,
        } => commands::test::execute(
            filter, watch, coverage, bail, timeout,
            concurrency, reporter, file, update_snapshots,
        ),

        Commands::Bench { filter, warmup, iterations, save, compare, json } =>
            commands::bench::execute(filter, warmup, iterations, save, compare, json),

        Commands::Fmt { files, check, diff, stdin } =>
            commands::fmt::execute(files, check, diff, stdin),

        Commands::Lint { files, fix, format, watch } =>
            commands::lint::execute(files, fix, format, watch),

        Commands::Repl { no_jit } =>
            commands::repl::execute(no_jit),

        Commands::Init { path, name, template, yes } =>
            commands::init::execute(path, name, template, yes),

        Commands::Add { package, dev, exact, no_install } =>
            commands::add::execute(package, dev, exact, no_install),

        Commands::Remove { package } =>
            commands::remove::execute(package),

        Commands::Install { production, frozen, force } =>
            commands::install::execute(production, frozen, force),

        Commands::Update { package } =>
            commands::update::execute(package),

        Commands::Publish { tag, dry_run, access } =>
            commands::publish::execute(tag, dry_run, access),

        Commands::Pkg { command } =>
            commands::pkg::execute(command),

        Commands::Bundle { file, output, target, release, strip, compress, no_runtime } =>
            commands::bundle::execute(file, output, target, release, strip, compress, no_runtime),

        Commands::Doc { out_dir, serve, open, format } =>
            commands::doc::execute(out_dir, serve, open, format),

        Commands::Lsp { stdio, port } =>
            commands::lsp::execute(stdio, port),

        Commands::Completions { shell } =>
            commands::completions::execute(shell),

        Commands::Clean { cache, dist, all } =>
            commands::clean::execute(cache, dist, all),

        Commands::Info =>
            commands::info::execute(),

        Commands::Upgrade { version, check, force } =>
            commands::upgrade::execute(version, check, force),
    }
}
