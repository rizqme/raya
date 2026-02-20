//! `raya bundle` — Compile Raya source to native AOT bundle.
//!
//! Requires the `aot` feature flag. Build with:
//! ```sh
//! cargo build -p raya-cli --features aot
//! ```
//!
//! Pipeline:
//! 1. Compile source to bytecode Module
//! 2. Lift bytecode functions to AOT-compilable form
//! 3. Compile through Cranelift to native machine code
//! 4. Write bundle format: [code section][func table][VFS][trailer]

// When AOT feature is not available, provide a stub that prints an error.
#[cfg(not(feature = "aot"))]
pub fn execute(
    _file: String,
    _output: String,
    _target: String,
    _release: bool,
    _strip: bool,
    _compress: bool,
    _no_runtime: bool,
) -> anyhow::Result<()> {
    anyhow::bail!(
        "The `bundle` command requires AOT compilation support.\n\
         Rebuild with: cargo build -p raya-cli --features aot"
    );
}

#[cfg(feature = "aot")]
pub fn execute(
    file: String,
    output: String,
    target: String,
    release: bool,
    strip: bool,
    compress: bool,
    no_runtime: bool,
) -> anyhow::Result<()> {
    aot_impl::execute_bundle(file, output, target, release, strip, compress, no_runtime)
}

#[cfg(feature = "aot")]
mod aot_impl {
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use raya_engine::aot::bytecode_adapter::LiftedFunction;
    use raya_engine::aot::codegen::{compile_functions, create_native_isa, CompilableFunction};
    use raya_engine::aot::traits::AotCompilable;
    use raya_runtime::bundle::format::{
        AotTrailer, BundledFuncEntry, TRAILER_MAGIC, TRAILER_SIZE, write_vfs_section,
    };
    use raya_runtime::Runtime;

    pub fn execute_bundle(
        file: String,
        output: String,
        target: String,
        release: bool,
        strip: bool,
        compress: bool,
        no_runtime: bool,
    ) -> anyhow::Result<()> {
        let _ = (strip, compress, no_runtime); // TODO: wire these flags

        if target != "native" {
            anyhow::bail!(
                "Cross-compilation is not yet supported. Only --target native is available."
            );
        }

        let src_path = PathBuf::from(&file);
        if !src_path.exists() {
            anyhow::bail!("Source file not found: {}", file);
        }

        // Determine output path
        let out_path = if output.is_empty() {
            let stem = src_path.file_stem().unwrap_or_default().to_string_lossy();
            PathBuf::from(format!("{}.bundle", stem))
        } else {
            PathBuf::from(&output)
        };

        println!("Compiling {} to native bundle...", src_path.display());

        // Step 1: Compile source to bytecode
        let rt = Runtime::new();
        let is_bytecode = src_path.extension().and_then(|e| e.to_str()) == Some("ryb");

        let compiled = if is_bytecode {
            rt.load_bytecode(&src_path)
                .map_err(|e| anyhow::anyhow!("Failed to load bytecode: {}", e))?
        } else {
            rt.compile_file(&src_path)
                .map_err(|e| anyhow::anyhow!("Failed to compile: {}", e))?
        };

        let module = compiled.module();
        let func_count = module.functions.len();
        println!("  Compiled {} function(s) to bytecode", func_count);

        if func_count == 0 {
            anyhow::bail!("No functions found in module. Nothing to bundle.");
        }

        // Step 2: Lift bytecode functions to AOT-compilable form
        let lifted: Vec<LiftedFunction> = module
            .functions
            .iter()
            .enumerate()
            .map(|(i, f)| LiftedFunction {
                func_index: i as u32,
                param_count: f.param_count as u32,
                local_count: f.local_count as u32,
                name: Some(f.name.clone()),
            })
            .collect();

        let compilables: Vec<CompilableFunction<'_>> = lifted
            .iter()
            .enumerate()
            .map(|(i, lf)| CompilableFunction {
                func: lf as &dyn AotCompilable,
                module_index: 0,
                func_index: i as u16,
            })
            .collect();

        // Step 3: Compile to native machine code via Cranelift
        println!(
            "  Compiling {} function(s) to native code...",
            compilables.len()
        );
        let isa = create_native_isa()
            .map_err(|e| anyhow::anyhow!("Failed to create native ISA: {}", e))?;

        let aot_bundle = compile_functions(&compilables, isa)
            .map_err(|e| anyhow::anyhow!("AOT compilation failed: {}", e))?;

        println!(
            "  Generated {} bytes of machine code ({} function(s))",
            aot_bundle.code_size(),
            aot_bundle.function_count()
        );

        // Step 4: Collect VFS files (embed bytecode for fallback)
        let mut vfs_files: Vec<(String, Vec<u8>)> = Vec::new();

        // Embed the bytecode module for runtime fallback
        let bytecode = compiled.encode();
        let module_name = module.metadata.name.clone();
        let vfs_name = if module_name.is_empty() {
            "main.ryb".to_string()
        } else {
            format!("{}.ryb", module_name)
        };
        vfs_files.push((vfs_name, bytecode));

        // Step 5: Write the bundle file
        write_bundle_file(&out_path, &aot_bundle, &vfs_files, release)?;

        let file_size = std::fs::metadata(&out_path)?.len();
        println!(
            "  Bundle written to {} ({} bytes)",
            out_path.display(),
            file_size
        );

        Ok(())
    }

    /// Write the AOT bundle to a file.
    ///
    /// Layout:
    /// ```text
    /// [Code Section]      — raw machine code blob
    /// [Function Table]    — array of BundledFuncEntry
    /// [VFS Section]       — embedded files
    /// [Trailer]           — fixed-size metadata at end
    /// ```
    fn write_bundle_file(
        path: &Path,
        bundle: &raya_engine::aot::codegen::AotBundle,
        vfs_files: &[(String, Vec<u8>)],
        _release: bool,
    ) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let mut file = std::fs::File::create(path)?;
        let payload_offset = 0u64; // Standalone bundle (no base binary)

        // 1. Write code section
        let code_offset = 0u64;
        file.write_all(&bundle.code)?;
        let code_size = bundle.code.len() as u64;

        // 2. Write function table
        let func_table_offset = code_offset + code_size;
        let func_table_count = bundle.func_table.len() as u32;

        for entry in &bundle.func_table {
            let bundled = BundledFuncEntry {
                global_func_id: entry.global_func_id.0,
                code_offset: entry.code_offset,
                code_size: entry.code_size,
                local_count: entry.local_count,
                param_count: entry.param_count,
            };
            file.write_all(&bundled.to_bytes())?;
        }

        // 3. Write VFS section
        let vfs_offset = func_table_offset
            + (func_table_count as u64) * std::mem::size_of::<BundledFuncEntry>() as u64;
        let vfs_size = write_vfs_section(&mut file, vfs_files)?;

        // 4. Compute checksum (FNV-1a hash of code section)
        let checksum = compute_checksum(&bundle.code);

        // 5. Write trailer
        let trailer = AotTrailer {
            magic: TRAILER_MAGIC,
            code_offset,
            code_size,
            func_table_offset,
            func_table_count,
            vfs_offset,
            vfs_size,
            target_triple: AotTrailer::encode_target_triple(&bundle.target_triple),
            checksum,
            trailer_size: TRAILER_SIZE as u32,
            payload_offset,
        };
        trailer.write_to(&mut file)?;

        file.flush()?;
        Ok(())
    }

    /// Simple checksum: FNV-1a hash of the data, truncated to u32.
    fn compute_checksum(data: &[u8]) -> u32 {
        let mut hash: u32 = 0x811c_9dc5;
        for &byte in data {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
        hash
    }
}
