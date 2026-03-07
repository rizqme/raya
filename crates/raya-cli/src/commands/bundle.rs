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
    use std::collections::{BTreeSet, VecDeque};
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use raya_engine::aot::bytecode_adapter::LiftedFunction;
    use raya_engine::aot::codegen::{compile_functions, create_native_isa, CompilableFunction};
    use raya_engine::aot::traits::AotCompilable;
    use raya_engine::compiler::bytecode::Opcode;
    use raya_runtime::bundle::format::{
        write_vfs_section, AotTrailer, BundledFuncEntry, TRAILER_MAGIC, TRAILER_SIZE,
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

        let reachable = collect_reachable_functions(module)?;
        println!(
            "  Selected {} reachable function(s) for AOT",
            reachable.len()
        );

        // Step 2: Lift bytecode functions to AOT-compilable form
        let lifted: Vec<LiftedFunction> = reachable
            .iter()
            .map(|&func_index| {
                let f = &module.functions[func_index as usize];
                let name = f.name.clone();
                let param_count = f.param_count as u32;
                let local_count = f.local_count as u32;
                LiftedFunction {
                    func_index,
                    param_count,
                    local_count,
                    name: Some(name.clone()),
                    #[cfg(all(feature = "aot", feature = "jit"))]
                    jit_func: raya_engine::jit::ir::JitFunction::new(
                        func_index,
                        name,
                        param_count as usize,
                        local_count as usize,
                    ),
                }
            })
            .collect();

        let compilables: Vec<CompilableFunction<'_>> = lifted
            .iter()
            .map(|lf| CompilableFunction {
                func: lf as &dyn AotCompilable,
                module_index: 0,
                func_index: lf.func_index as u16,
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

    fn collect_reachable_functions(
        module: &raya_engine::compiler::bytecode::Module,
    ) -> anyhow::Result<Vec<u32>> {
        let entry_main_fn_id = module
            .functions
            .iter()
            .rposition(|f| f.name == "main")
            .ok_or_else(|| anyhow::anyhow!("No main function"))?;

        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        seen.insert(entry_main_fn_id as u32);
        queue.push_back(entry_main_fn_id as u32);

        while let Some(func_id) = queue.pop_front() {
            let function = &module.functions[func_id as usize];
            let code = &function.code;
            let mut ip = 0usize;
            while ip < code.len() {
                let Some(opcode) = Opcode::from_u8(code[ip]) else {
                    break;
                };
                ip += 1;

                if opcode == Opcode::Call {
                    if ip + 6 > code.len() {
                        break;
                    }
                    let callee_fn_id =
                        u32::from_le_bytes([code[ip], code[ip + 1], code[ip + 2], code[ip + 3]]);
                    if (callee_fn_id as usize) < module.functions.len() && seen.insert(callee_fn_id)
                    {
                        queue.push_back(callee_fn_id);
                    }
                }

                let operand_len = operand_size(opcode);
                if ip + operand_len > code.len() {
                    break;
                }
                ip += operand_len;
            }
        }

        Ok(seen.into_iter().collect())
    }

    fn operand_size(opcode: Opcode) -> usize {
        match opcode {
            // No operands
            Opcode::Nop
            | Opcode::Pop
            | Opcode::Dup
            | Opcode::Swap
            | Opcode::ConstNull
            | Opcode::ConstTrue
            | Opcode::ConstFalse
            | Opcode::LoadLocal0
            | Opcode::LoadLocal1
            | Opcode::StoreLocal0
            | Opcode::StoreLocal1
            | Opcode::GetArgCount
            | Opcode::LoadArgLocal
            | Opcode::Iadd
            | Opcode::Isub
            | Opcode::Imul
            | Opcode::Idiv
            | Opcode::Imod
            | Opcode::Ineg
            | Opcode::Ipow
            | Opcode::Ishl
            | Opcode::Ishr
            | Opcode::Iushr
            | Opcode::Iand
            | Opcode::Ior
            | Opcode::Ixor
            | Opcode::Inot
            | Opcode::Fadd
            | Opcode::Fsub
            | Opcode::Fmul
            | Opcode::Fdiv
            | Opcode::Fneg
            | Opcode::Fpow
            | Opcode::Fmod
            | Opcode::Ieq
            | Opcode::Ine
            | Opcode::Ilt
            | Opcode::Ile
            | Opcode::Igt
            | Opcode::Ige
            | Opcode::Feq
            | Opcode::Fne
            | Opcode::Flt
            | Opcode::Fle
            | Opcode::Fgt
            | Opcode::Fge
            | Opcode::Eq
            | Opcode::Ne
            | Opcode::StrictEq
            | Opcode::StrictNe
            | Opcode::Not
            | Opcode::And
            | Opcode::Or
            | Opcode::Typeof
            | Opcode::Sconcat
            | Opcode::Slen
            | Opcode::Seq
            | Opcode::Sne
            | Opcode::Slt
            | Opcode::Sle
            | Opcode::Sgt
            | Opcode::Sge
            | Opcode::ToString
            | Opcode::Return
            | Opcode::ReturnVoid
            | Opcode::LoadElem
            | Opcode::StoreElem
            | Opcode::ArrayLen
            | Opcode::Await
            | Opcode::Yield
            | Opcode::Sleep
            | Opcode::NewMutex
            | Opcode::NewChannel
            | Opcode::MutexLock
            | Opcode::MutexUnlock
            | Opcode::Throw
            | Opcode::DynGetKeyed
            | Opcode::DynSetKeyed
            | Opcode::DynNewObject
            | Opcode::DynKeys
            | Opcode::NewSemaphore
            | Opcode::SemAcquire
            | Opcode::SemRelease
            | Opcode::WaitAll
            | Opcode::TaskCancel
            | Opcode::NewRefCell
            | Opcode::LoadRefCell
            | Opcode::StoreRefCell
            | Opcode::ArrayPush
            | Opcode::ArrayPop
            | Opcode::TupleGet
            | Opcode::EndTry
            | Opcode::Rethrow
            | Opcode::Debugger => 0,
            Opcode::BindMethod
            | Opcode::LoadLocal
            | Opcode::StoreLocal
            | Opcode::LoadFieldExact
            | Opcode::StoreFieldExact
            | Opcode::OptionalFieldExact
            | Opcode::InitObject
            | Opcode::InitArray
            | Opcode::InitTuple
            | Opcode::CloseVar
            | Opcode::LoadCaptured
            | Opcode::StoreCaptured
            | Opcode::SetClosureCapture
            | Opcode::Trap
            | Opcode::SpawnClosure
            | Opcode::NewType
            | Opcode::IsNominal
            | Opcode::CastTupleLen
            | Opcode::CastObjectMinFields
            | Opcode::CastArrayElemKind
            | Opcode::CastKindMask
            | Opcode::Cast
            | Opcode::CastNominal => 2,
            Opcode::ConstI32
            | Opcode::Jmp
            | Opcode::JmpIfFalse
            | Opcode::JmpIfTrue
            | Opcode::JmpIfNull
            | Opcode::JmpIfNotNull
            | Opcode::ConstStr
            | Opcode::LoadConst
            | Opcode::LoadGlobal
            | Opcode::StoreGlobal
            | Opcode::NewArray
            | Opcode::LoadModule
            | Opcode::TaskThen
            | Opcode::DynGet
            | Opcode::DynSet
            | Opcode::DynDelete
            | Opcode::DynHas
            | Opcode::LoadStatic
            | Opcode::StoreStatic => 4,
            Opcode::LoadFieldShape | Opcode::StoreFieldShape | Opcode::OptionalFieldShape => 10,
            Opcode::CallMethodShape | Opcode::OptionalCallMethodShape => 12,
            Opcode::ConstF64
            | Opcode::CastShape
            | Opcode::ImplementsShape
            | Opcode::ArrayLiteral
            | Opcode::Try => 8,
            Opcode::Call
            | Opcode::CallMethodExact
            | Opcode::OptionalCallMethodExact
            | Opcode::CallConstructor
            | Opcode::CallSuper
            | Opcode::CallStatic
            | Opcode::ObjectLiteral
            | Opcode::Spawn
            | Opcode::MakeClosure
            | Opcode::TupleLiteral => 6,
            Opcode::ConstructType => 3,
            Opcode::NativeCall | Opcode::ModuleNativeCall => 3,
        }
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

        // 4. Compute checksum over full payload ([code][func_table][vfs]).
        // Loader validates crc32(payload_start..payload_end), i.e. all bytes
        // before trailer for standalone bundles.
        file.flush()?;
        let payload_bytes = std::fs::read(path)?;
        let checksum = crc32fast::hash(&payload_bytes);

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
}
