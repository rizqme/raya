use crate::vm::object::{
    BindingState, BindingStorageKind, EnvBinding, EnvRecordData, EnvRecordKind,
};
use crate::vm::value::Value;
use rustc_hash::FxHashMap;

pub(crate) const DIRECT_EVAL_OUTER_ENV_KEY: &str = "__direct_eval_outer_env__";
pub(crate) const DIRECT_EVAL_COMPLETION_KEY: &str = "__direct_eval_completion__";
pub(crate) const DIRECT_EVAL_LOCALS_BASE_KEY: &str = "__direct_eval_locals_base__";
pub(crate) const DIRECT_EVAL_LEXICAL_MARKER_PREFIX: &str = "__direct_eval_lexical__:";
pub(crate) const DIRECT_EVAL_UNINITIALIZED_MARKER_PREFIX: &str = "__direct_eval_uninitialized__:";
pub(crate) const DIRECT_EVAL_OUTER_SNAPSHOT_MARKER_PREFIX: &str =
    "__direct_eval_outer_snapshot__:";
pub(crate) const DIRECT_EVAL_LOCAL_SLOT_PREFIX: &str = "__direct_eval_local_slot__:";
pub(crate) const DIRECT_EVAL_LOCAL_REFCELL_PREFIX: &str = "__direct_eval_local_refcell__:";
pub(crate) const WITH_ENV_TARGET_KEY: &str = "__with_env_target__";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JsEnvFrameKind {
    GlobalScript,
    Declarative,
    WithObject,
    DirectEvalActivation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JsBindingKind {
    Var,
    Function,
    LexicalMutable,
    LexicalImmutable,
    Parameter,
    NamedFunctionSelf,
    Arguments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JsBindingStorage {
    RuntimeSlot,
    LocalSlot { index: u32 },
    RefCellLocal { index: u32 },
    CaptureSlot { index: u32 },
    GlobalPublished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsEvalCompletion {
    Empty,
    Value(Value),
}

impl Default for JsEvalCompletion {
    fn default() -> Self {
        Self::Empty
    }
}

impl JsEvalCompletion {
    pub fn into_option(self) -> Option<Value> {
        match self {
            Self::Empty => None,
            Self::Value(value) => Some(value),
        }
    }

    pub fn from_option(value: Option<Value>) -> Self {
        value.map(Self::Value).unwrap_or(Self::Empty)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsBindingSpec {
    pub name: String,
    pub kind: JsBindingKind,
    pub storage: JsBindingStorage,
    pub initialized: bool,
    pub deletable: bool,
    pub strict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JsEnvManifest {
    pub frame_kind: Option<JsEnvFrameKind>,
    pub bindings: Vec<JsBindingSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsEnvFrame {
    pub kind: JsEnvFrameKind,
    pub handle: Option<Value>,
    pub outer: Option<Value>,
    pub with_target: Option<Value>,
    pub bindings: FxHashMap<String, JsBindingSpec>,
}

impl JsEnvFrame {
    pub fn new(kind: JsEnvFrameKind) -> Self {
        Self {
            kind,
            handle: None,
            outer: None,
            with_target: None,
            bindings: FxHashMap::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JsEnvStack {
    pub frames: Vec<JsEnvFrame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JsExecutionContext {
    pub lexical_env: Option<Value>,
    pub variable_env: Option<Value>,
    pub this_value: Option<Value>,
    pub super_home_object: Option<Value>,
    pub new_target: Option<Value>,
    pub strict: bool,
    pub uses_script_global_bindings: bool,
    pub persist_caller_declarations: bool,
    pub eval_completion: JsEvalCompletion,
}

impl JsExecutionContext {
    pub fn direct_eval(
        env: Value,
        strict: bool,
        uses_script_global_bindings: bool,
        persist_caller_declarations: bool,
    ) -> Self {
        Self {
            lexical_env: Some(env),
            variable_env: Some(env),
            this_value: None,
            super_home_object: None,
            new_target: None,
            strict,
            uses_script_global_bindings,
            persist_caller_declarations,
            eval_completion: JsEvalCompletion::Empty,
        }
    }

    pub fn with_direct_eval_env(
        mut self,
        env: Value,
        strict: bool,
        uses_script_global_bindings: bool,
        persist_caller_declarations: bool,
    ) -> Self {
        self.lexical_env = Some(env);
        self.variable_env = Some(env);
        self.strict = strict;
        self.uses_script_global_bindings = uses_script_global_bindings;
        self.persist_caller_declarations = persist_caller_declarations;
        self
    }

    pub fn with_super_home_object(mut self, home_object: Value) -> Self {
        self.super_home_object = Some(home_object);
        self
    }

    pub fn with_new_target(mut self, new_target: Value) -> Self {
        self.new_target = Some(new_target);
        self
    }

    pub fn with_eval_completion(mut self, completion: Option<Value>) -> Self {
        self.eval_completion = JsEvalCompletion::from_option(completion);
        self
    }
}

pub(crate) fn direct_eval_lexical_marker_key(name: &str) -> String {
    format!("{DIRECT_EVAL_LEXICAL_MARKER_PREFIX}{name}")
}

pub(crate) fn direct_eval_uninitialized_marker_key(name: &str) -> String {
    format!("{DIRECT_EVAL_UNINITIALIZED_MARKER_PREFIX}{name}")
}

pub(crate) fn direct_eval_outer_snapshot_marker_key(name: &str) -> String {
    format!("{DIRECT_EVAL_OUTER_SNAPSHOT_MARKER_PREFIX}{name}")
}

pub(crate) fn direct_eval_local_slot_key(name: &str) -> String {
    format!("{DIRECT_EVAL_LOCAL_SLOT_PREFIX}{name}")
}

pub(crate) fn direct_eval_local_refcell_key(name: &str) -> String {
    format!("{DIRECT_EVAL_LOCAL_REFCELL_PREFIX}{name}")
}

pub(crate) fn is_legacy_env_metadata_key(key: &str) -> bool {
    key == DIRECT_EVAL_OUTER_ENV_KEY
        || key == DIRECT_EVAL_COMPLETION_KEY
        || key == DIRECT_EVAL_LOCALS_BASE_KEY
        || key == WITH_ENV_TARGET_KEY
        || key.starts_with(DIRECT_EVAL_LEXICAL_MARKER_PREFIX)
        || key.starts_with(DIRECT_EVAL_UNINITIALIZED_MARKER_PREFIX)
        || key.starts_with(DIRECT_EVAL_OUTER_SNAPSHOT_MARKER_PREFIX)
        || key.starts_with(DIRECT_EVAL_LOCAL_SLOT_PREFIX)
        || key.starts_with(DIRECT_EVAL_LOCAL_REFCELL_PREFIX)
}

pub(crate) fn build_legacy_env_record<I, F>(
    default_kind: EnvRecordKind,
    property_names: I,
    mut lookup: F,
) -> EnvRecordData
where
    I: IntoIterator<Item = String>,
    F: FnMut(&str) -> Option<Value>,
{
    let mut record = EnvRecordData::new(default_kind);
    record.outer = lookup(DIRECT_EVAL_OUTER_ENV_KEY);
    record.with_target = lookup(WITH_ENV_TARGET_KEY);
    record.completion = lookup(DIRECT_EVAL_COMPLETION_KEY).unwrap_or(Value::undefined());
    record.locals_base = lookup(DIRECT_EVAL_LOCALS_BASE_KEY)
        .and_then(|value| value.as_i32())
        .filter(|base| *base >= 0)
        .map(|base| base as usize);

    for key in property_names {
        if is_legacy_env_metadata_key(&key) {
            continue;
        }

        let lexical = lookup(&direct_eval_lexical_marker_key(&key))
            .is_some_and(|value| value.is_truthy());
        let uninitialized = lookup(&direct_eval_uninitialized_marker_key(&key))
            .is_some_and(|value| value.is_truthy());
        let local_slot = lookup(&direct_eval_local_slot_key(&key))
            .and_then(|value| value.as_i32())
            .filter(|slot| *slot >= 0)
            .map(|slot| slot as u16);
        let local_refcell = lookup(&direct_eval_local_refcell_key(&key))
            .is_some_and(|value| value.is_truthy());
        let outer_snapshot = lookup(&direct_eval_outer_snapshot_marker_key(&key))
            .is_some_and(|value| value.is_truthy());
        let storage = match (local_slot, local_refcell) {
            (Some(_), true) => BindingStorageKind::LocalRefCellSlot,
            (Some(_), false) => BindingStorageKind::LocalSlot,
            (None, _) => BindingStorageKind::Value,
        };
        let binding_value = match local_slot {
            Some(slot) => Value::i32(slot as i32),
            None => lookup(&key).unwrap_or(Value::undefined()),
        };
        record.bindings.insert(
            key,
            EnvBinding {
                storage,
                state: if uninitialized {
                    BindingState::Uninitialized
                } else {
                    BindingState::Initialized
                },
                value: binding_value,
                lexical,
                outer_snapshot,
                deletable: !lexical,
                strict: lexical,
            },
        );
    }

    record
}

impl JsEnvManifest {
    pub fn frame_kind_or(self_kind: &Self, default: JsEnvFrameKind) -> JsEnvFrameKind {
        self_kind.frame_kind.unwrap_or(default)
    }

    pub fn binding_specs_by_name(&self) -> FxHashMap<String, JsBindingSpec> {
        self.bindings
            .iter()
            .cloned()
            .map(|binding| (binding.name.clone(), binding))
            .collect()
    }
}

impl JsEnvStack {
    pub fn push_frame_from_manifest(
        &mut self,
        manifest: &JsEnvManifest,
        default_kind: JsEnvFrameKind,
        handle: Option<Value>,
        outer: Option<Value>,
        with_target: Option<Value>,
    ) {
        self.frames.push(JsEnvFrame::from_manifest(
            manifest,
            default_kind,
            handle,
            outer,
            with_target,
        ));
    }

    pub fn replace_top_from_manifest(
        &mut self,
        manifest: &JsEnvManifest,
        default_kind: JsEnvFrameKind,
        handle: Option<Value>,
        outer: Option<Value>,
        with_target: Option<Value>,
    ) -> Option<JsEnvFrame> {
        let frame = JsEnvFrame::from_manifest(manifest, default_kind, handle, outer, with_target);
        if let Some(current) = self.frames.last_mut() {
            Some(std::mem::replace(current, frame))
        } else {
            self.frames.push(frame);
            None
        }
    }
}

impl JsEnvFrame {
    pub fn from_manifest(
        manifest: &JsEnvManifest,
        default_kind: JsEnvFrameKind,
        handle: Option<Value>,
        outer: Option<Value>,
        with_target: Option<Value>,
    ) -> Self {
        Self {
            kind: manifest.frame_kind.unwrap_or(default_kind),
            handle,
            outer,
            with_target,
            bindings: manifest.binding_specs_by_name(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsReferenceRecord {
    Identifier,
    Property,
    SuperProperty,
}

#[cfg(test)]
mod tests {
    use super::{
        JsBindingKind, JsBindingSpec, JsBindingStorage, JsEnvFrame, JsEnvFrameKind, JsEnvManifest,
        JsEnvStack,
    };

    fn binding(name: &str, kind: JsBindingKind) -> JsBindingSpec {
        JsBindingSpec {
            name: name.to_string(),
            kind,
            storage: JsBindingStorage::RuntimeSlot,
            initialized: true,
            deletable: false,
            strict: true,
        }
    }

    #[test]
    fn frame_from_manifest_uses_binding_specs() {
        let manifest = JsEnvManifest {
            frame_kind: Some(JsEnvFrameKind::Declarative),
            bindings: vec![
                binding("a", JsBindingKind::LexicalMutable),
                binding("b", JsBindingKind::Parameter),
            ],
        };

        let frame = JsEnvFrame::from_manifest(
            &manifest,
            JsEnvFrameKind::GlobalScript,
            None,
            None,
            None,
        );
        assert_eq!(frame.kind, JsEnvFrameKind::Declarative);
        assert_eq!(frame.bindings.len(), 2);
        assert_eq!(
            frame.bindings.get("a").map(|binding| binding.kind),
            Some(JsBindingKind::LexicalMutable)
        );
        assert_eq!(
            frame.bindings.get("b").map(|binding| binding.kind),
            Some(JsBindingKind::Parameter)
        );
    }

    #[test]
    fn replace_top_from_manifest_replaces_existing_frame() {
        let mut stack = JsEnvStack::default();
        let first = JsEnvManifest {
            frame_kind: Some(JsEnvFrameKind::GlobalScript),
            bindings: vec![binding("g", JsBindingKind::Var)],
        };
        let second = JsEnvManifest {
            frame_kind: Some(JsEnvFrameKind::WithObject),
            bindings: vec![binding("x", JsBindingKind::LexicalMutable)],
        };

        stack.push_frame_from_manifest(&first, JsEnvFrameKind::Declarative, None, None, None);
        let replaced =
            stack.replace_top_from_manifest(&second, JsEnvFrameKind::Declarative, None, None, None);

        assert_eq!(
            replaced.as_ref().map(|frame| frame.kind),
            Some(JsEnvFrameKind::GlobalScript)
        );
        assert_eq!(stack.frames.len(), 1);
        assert_eq!(stack.frames[0].kind, JsEnvFrameKind::WithObject);
        assert!(stack.frames[0].bindings.contains_key("x"));
        assert!(!stack.frames[0].bindings.contains_key("g"));
    }
}
