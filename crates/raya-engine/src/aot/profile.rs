use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
pub enum AotSiteKind {
    LoadFieldShape,
    StoreFieldShape,
    ImplementsShape,
    CastShape,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AotHotLayout {
    pub layout_id: u32,
    pub hits: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AotSiteProfile {
    pub bytecode_offset: u32,
    pub kind: AotSiteKind,
    pub layouts: Vec<AotHotLayout>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AotFunctionProfile {
    pub func_index: u32,
    pub call_count: u64,
    pub loop_count: u64,
    pub sites: Vec<AotSiteProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AotModuleProfile {
    pub checksum: [u8; 32],
    pub functions: Vec<AotFunctionProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AotProfileData {
    pub modules: Vec<AotModuleProfile>,
}

impl AotProfileData {
    pub fn function_profile(
        &self,
        checksum: &[u8; 32],
        func_index: u32,
    ) -> Option<&AotFunctionProfile> {
        self.modules
            .iter()
            .find(|module| &module.checksum == checksum)
            .and_then(|module| {
                module
                    .functions
                    .iter()
                    .find(|func| func.func_index == func_index)
            })
    }

    pub fn function_hotness(&self, checksum: &[u8; 32], func_index: u32) -> u64 {
        self.function_profile(checksum, func_index)
            .map(|func| {
                func.call_count
                    .saturating_add(func.loop_count.saturating_mul(4))
            })
            .unwrap_or(0)
    }
}

#[derive(Debug, Default)]
pub struct AotProfileCollector {
    modules: FxHashMap<[u8; 32], ModuleProfileAccumulator>,
}

#[derive(Debug, Default)]
struct ModuleProfileAccumulator {
    functions: FxHashMap<u32, FunctionProfileAccumulator>,
}

#[derive(Debug, Default)]
struct FunctionProfileAccumulator {
    call_count: u64,
    loop_count: u64,
    sites: FxHashMap<(u32, AotSiteKind), FxHashMap<u32, u64>>,
}

impl AotProfileCollector {
    pub fn record_call(&mut self, checksum: [u8; 32], func_index: u32) {
        self.modules
            .entry(checksum)
            .or_default()
            .functions
            .entry(func_index)
            .or_default()
            .call_count += 1;
    }

    pub fn record_loop(&mut self, checksum: [u8; 32], func_index: u32) {
        self.modules
            .entry(checksum)
            .or_default()
            .functions
            .entry(func_index)
            .or_default()
            .loop_count += 1;
    }

    pub fn record_layout_site(
        &mut self,
        checksum: [u8; 32],
        func_index: u32,
        bytecode_offset: u32,
        kind: AotSiteKind,
        layout_id: u32,
    ) {
        *self
            .modules
            .entry(checksum)
            .or_default()
            .functions
            .entry(func_index)
            .or_default()
            .sites
            .entry((bytecode_offset, kind))
            .or_default()
            .entry(layout_id)
            .or_insert(0) += 1;
    }

    pub fn snapshot(&self) -> AotProfileData {
        let mut modules = self
            .modules
            .iter()
            .map(|(checksum, module)| {
                let mut functions = module
                    .functions
                    .iter()
                    .map(|(func_index, func)| {
                        let mut sites = func
                            .sites
                            .iter()
                            .map(|((bytecode_offset, kind), layouts)| {
                                let mut layouts = layouts
                                    .iter()
                                    .map(|(layout_id, hits)| AotHotLayout {
                                        layout_id: *layout_id,
                                        hits: *hits,
                                    })
                                    .collect::<Vec<_>>();
                                layouts.sort_by_key(|entry| std::cmp::Reverse(entry.hits));
                                AotSiteProfile {
                                    bytecode_offset: *bytecode_offset,
                                    kind: *kind,
                                    layouts,
                                }
                            })
                            .collect::<Vec<_>>();
                        sites.sort_by_key(|site| (site.bytecode_offset, site.kind));
                        AotFunctionProfile {
                            func_index: *func_index,
                            call_count: func.call_count,
                            loop_count: func.loop_count,
                            sites,
                        }
                    })
                    .collect::<Vec<_>>();
                functions.sort_by_key(|func| func.func_index);
                AotModuleProfile {
                    checksum: *checksum,
                    functions,
                }
            })
            .collect::<Vec<_>>();
        modules.sort_by_key(|module| module.checksum);
        AotProfileData { modules }
    }
}
