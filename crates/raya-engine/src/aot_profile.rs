#[cfg(feature = "aot")]
pub use crate::aot::profile::{AotProfileCollector, AotProfileData, AotSiteKind};

#[cfg(not(feature = "aot"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AotSiteKind {
    LoadFieldShape,
    StoreFieldShape,
    CastShape,
    ImplementsShape,
}

#[cfg(not(feature = "aot"))]
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AotProfileData;

#[cfg(not(feature = "aot"))]
#[derive(Debug, Default)]
pub struct AotProfileCollector;

#[cfg(not(feature = "aot"))]
impl AotProfileCollector {
    pub fn record_call(&mut self, _checksum: [u8; 32], _func_index: u32) {}

    pub fn record_loop(&mut self, _checksum: [u8; 32], _func_index: u32) {}

    pub fn record_layout_site(
        &mut self,
        _checksum: [u8; 32],
        _func_index: u32,
        _bytecode_offset: u32,
        _kind: AotSiteKind,
        _layout_id: crate::vm::object::LayoutId,
    ) {
    }

    pub fn snapshot(&self) -> AotProfileData {
        AotProfileData
    }
}
