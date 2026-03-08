//! Canonical structural type signatures for cross-module compatibility.
//!
//! The canonical form is deterministic and intentionally ignores declaration
//! ordering where order is semantically irrelevant (object/class/interface
//! member declarations). Function parameter order and tuple order are preserved.

use super::context::TypeContext;
use super::subtyping::SubtypingContext;
use super::ty::{PrimitiveType, Type, TypeId};
use crate::parser::ast::Visibility;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Structural signature payload for a type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalTypeSignature {
    /// Deterministic canonical structural signature string.
    pub canonical: String,
    /// Stable 64-bit hash of `canonical`.
    pub hash: u64,
}

/// Compute canonical structural signature + hash for a type.
pub fn canonical_type_signature(type_id: TypeId, type_ctx: &TypeContext) -> CanonicalTypeSignature {
    let mut canon = Canonicalizer::new(type_ctx);
    let canonical = canon.canonicalize_type(type_id);
    let hash = signature_hash(&canonical);
    CanonicalTypeSignature { canonical, hash }
}

/// Compute only the canonical structural hash for a type.
pub fn type_signature_hash(type_id: TypeId, type_ctx: &TypeContext) -> u64 {
    canonical_type_signature(type_id, type_ctx).hash
}

/// Compute only the canonical structural signature string for a type.
pub fn type_signature_string(type_id: TypeId, type_ctx: &TypeContext) -> String {
    canonical_type_signature(type_id, type_ctx).canonical
}

/// Stable hash for canonical signatures.
pub fn signature_hash(signature: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(signature.as_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ])
}

/// Intern a canonical structural signature into a `TypeContext`.
///
/// This is used when importing symbols from other modules: exported `TypeId`s
/// are context-local, so we hydrate the canonical signature into the current
/// module's `TypeContext` instead.
pub fn hydrate_type_from_canonical_signature(
    signature: &str,
    type_ctx: &mut TypeContext,
) -> TypeId {
    try_hydrate_type_from_canonical_signature(signature, type_ctx)
        .unwrap_or_else(|| type_ctx.any_type())
}

/// Attempt to intern a canonical structural signature into a `TypeContext`.
///
/// Returns `None` when the signature cannot be parsed.
pub fn try_hydrate_type_from_canonical_signature(
    signature: &str,
    type_ctx: &mut TypeContext,
) -> Option<TypeId> {
    let mut parser = SignatureHydrator::new(type_ctx);
    parser.parse_type(signature)
}

/// Check whether `actual_signature` can satisfy `expected_signature`.
///
/// Compatibility direction is structural assignability: `actual <: expected`.
/// This allows structural subsets such as:
/// - object width subtyping (`{a,b,c}` satisfies `{a,b}`)
/// - union subset compatibility (`number` satisfies `number|string`)
/// - relaxed callable arity where extra call args may be ignored by callees.
pub fn structural_signature_is_assignable(
    expected_signature: &str,
    actual_signature: &str,
) -> bool {
    let mut type_ctx = TypeContext::new();
    let Some(expected_ty) =
        try_hydrate_type_from_canonical_signature(expected_signature, &mut type_ctx)
    else {
        return false;
    };
    let Some(actual_ty) =
        try_hydrate_type_from_canonical_signature(actual_signature, &mut type_ctx)
    else {
        return false;
    };

    let mut subtyping = SubtypingContext::new(&type_ctx).with_relaxed_function_call_arity(true);
    subtyping.is_subtype(actual_ty, expected_ty)
}

struct Canonicalizer<'a> {
    type_ctx: &'a TypeContext,
    /// Active recursion markers (`TypeId` -> recursion variable index).
    active: HashMap<TypeId, usize>,
    /// Type-variable alpha-renaming stack.
    type_var_scopes: Vec<HashMap<String, String>>,
    next_type_var: usize,
}

impl<'a> Canonicalizer<'a> {
    fn new(type_ctx: &'a TypeContext) -> Self {
        Self {
            type_ctx,
            active: HashMap::new(),
            type_var_scopes: Vec::new(),
            next_type_var: 0,
        }
    }

    fn canonicalize_type(&mut self, type_id: TypeId) -> String {
        if let Some(rec_idx) = self.active.get(&type_id).copied() {
            return format!("@R{}", rec_idx);
        }

        let rec_idx = self.active.len();
        self.active.insert(type_id, rec_idx);
        let result = self.canonicalize_type_inner(type_id);
        self.active.remove(&type_id);
        result
    }

    fn canonicalize_type_inner(&mut self, type_id: TypeId) -> String {
        let Some(ty) = self.type_ctx.get(type_id) else {
            return format!("invalid#{}", type_id.as_u32());
        };

        match ty {
            Type::Primitive(p) => canonical_primitive(*p).to_string(),
            Type::Never => "never".to_string(),
            Type::Any => "any".to_string(),
            Type::Unknown => "unknown".to_string(),
            Type::JSObject => "js_object".to_string(),
            Type::Mutex => "Mutex".to_string(),
            Type::RegExp => "RegExp".to_string(),
            Type::Date => "Date".to_string(),
            Type::Buffer => "Buffer".to_string(),
            Type::Json => "json".to_string(),

            Type::StringLiteral(v) => format!("strlit({})", escape(v)),
            Type::NumberLiteral(v) => format!("numlit({:x})", v.to_bits()),
            Type::BooleanLiteral(v) => format!("boollit({})", if *v { "1" } else { "0" }),

            Type::Array(array) => format!("arr({})", self.canonicalize_type(array.element)),
            Type::Task(task) => format!("Promise<{}>", self.canonicalize_type(task.result)),
            Type::Channel(chan) => format!("Channel<{}>", self.canonicalize_type(chan.message)),
            Type::Map(map) => format!(
                "Map<{},{}>",
                self.canonicalize_type(map.key),
                self.canonicalize_type(map.value)
            ),
            Type::Set(set) => format!("Set<{}>", self.canonicalize_type(set.element)),
            Type::Tuple(tuple) => {
                let elems = tuple
                    .elements
                    .iter()
                    .map(|elem| self.canonicalize_type(*elem))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("tuple({})", elems)
            }
            Type::Object(object) => self.canonicalize_object(object),
            Type::Union(union) => self.canonicalize_union_members(&union.members),
            Type::Function(func) => self.canonicalize_function(
                func.min_params,
                &func.params,
                func.rest_param,
                func.return_type,
                func.is_async,
            ),
            Type::Class(class_ty) => self.canonicalize_class(class_ty),
            Type::Interface(iface) => self.canonicalize_interface(iface),
            Type::TypeVar(tv) => self.canonicalize_type_var(tv),
            Type::Generic(generic) => {
                let base = self.canonicalize_type(generic.base);
                let args = generic
                    .type_args
                    .iter()
                    .map(|arg| self.canonicalize_type(*arg))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("generic({},[{}])", base, args)
            }
            Type::Reference(reference) => {
                // Prefer structural projection when a named type exists in the context.
                if reference.type_args.is_none() {
                    if let Some(named_id) = self.type_ctx.lookup_named_type(&reference.name) {
                        return format!("alias({})", self.canonicalize_type(named_id));
                    }
                }
                let args = reference
                    .type_args
                    .as_ref()
                    .map(|type_args| {
                        type_args
                            .iter()
                            .map(|arg| self.canonicalize_type(*arg))
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();
                format!("ref({},[{}])", escape(&reference.name), args)
            }
            Type::Keyof(keyof) => format!("keyof({})", self.canonicalize_type(keyof.target)),
            Type::IndexedAccess(indexed) => format!(
                "index({}, {})",
                self.canonicalize_type(indexed.object),
                self.canonicalize_type(indexed.index)
            ),
        }
    }

    fn canonicalize_union_members(&mut self, members: &[TypeId]) -> String {
        let mut flattened = Vec::new();
        let mut seen = HashSet::new();
        self.collect_union_members(members, &mut flattened, &mut seen);
        flattened.sort_unstable();
        format!("union({})", flattened.join("|"))
    }

    fn collect_union_members(
        &mut self,
        members: &[TypeId],
        out: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) {
        for member in members {
            if let Some(Type::Union(union)) = self.type_ctx.get(*member) {
                self.collect_union_members(&union.members, out, seen);
                continue;
            }

            let canonical = self.canonicalize_type(*member);
            if seen.insert(canonical.clone()) {
                out.push(canonical);
            }
        }
    }

    fn canonicalize_object(&mut self, object: &super::ty::ObjectType) -> String {
        let mut members = BTreeSet::new();

        for property in &object.properties {
            let readonly = if property.readonly { "ro" } else { "rw" };
            let optional = if property.optional { "opt" } else { "req" };
            let entry = format!(
                "prop:{}:{}:{}:{}",
                escape(&property.name),
                readonly,
                optional,
                self.canonicalize_type(property.ty)
            );
            members.insert(entry);
        }

        if let Some((index_key, index_ty)) = &object.index_signature {
            members.insert(format!(
                "index:{}:{}",
                escape(index_key),
                self.canonicalize_type(*index_ty)
            ));
        }

        for sig in &object.call_signatures {
            members.insert(format!("call:{}", self.canonicalize_type(*sig)));
        }

        for sig in &object.construct_signatures {
            members.insert(format!("ctor:{}", self.canonicalize_type(*sig)));
        }

        format!("obj({})", members.into_iter().collect::<Vec<_>>().join(","))
    }

    fn canonicalize_function(
        &mut self,
        min_params: usize,
        params: &[TypeId],
        rest_param: Option<TypeId>,
        return_type: TypeId,
        is_async: bool,
    ) -> String {
        let params = params
            .iter()
            .map(|param| self.canonicalize_type(*param))
            .collect::<Vec<_>>()
            .join(",");
        let rest = rest_param
            .map(|rest| self.canonicalize_type(rest))
            .unwrap_or_else(|| "_".to_string());
        let ret = if is_async {
            let async_result = match self.type_ctx.get(return_type) {
                Some(Type::Task(task)) => task.result,
                _ => return_type,
            };
            format!("Promise<{}>", self.canonicalize_type(async_result))
        } else {
            self.canonicalize_type(return_type)
        };
        format!(
            "fn(min={},params=[{}],rest={},ret={})",
            min_params, params, rest, ret
        )
    }

    fn canonicalize_class(&mut self, class_ty: &super::ty::ClassType) -> String {
        self.with_alpha_params(&class_ty.type_params, |this| {
            let mut members = BTreeSet::new();

            for property in &class_ty.properties {
                if property.visibility != Visibility::Public {
                    continue;
                }
                let readonly = if property.readonly { "ro" } else { "rw" };
                let optional = if property.optional { "opt" } else { "req" };
                members.insert(format!(
                    "inst_prop:{}:{}:{}:{}",
                    escape(&property.name),
                    readonly,
                    optional,
                    this.canonicalize_type(property.ty)
                ));
            }

            for method in &class_ty.methods {
                if method.visibility != Visibility::Public {
                    continue;
                }
                let method_sig = this.with_alpha_params(&method.type_params, |this| {
                    this.canonicalize_type(method.ty)
                });
                members.insert(format!(
                    "inst_method:{}:{}",
                    escape(&method.name),
                    method_sig
                ));
            }

            for property in &class_ty.static_properties {
                if property.visibility != Visibility::Public {
                    continue;
                }
                let readonly = if property.readonly { "ro" } else { "rw" };
                let optional = if property.optional { "opt" } else { "req" };
                members.insert(format!(
                    "static_prop:{}:{}:{}:{}",
                    escape(&property.name),
                    readonly,
                    optional,
                    this.canonicalize_type(property.ty)
                ));
            }

            for method in &class_ty.static_methods {
                if method.visibility != Visibility::Public {
                    continue;
                }
                let method_sig = this.with_alpha_params(&method.type_params, |this| {
                    this.canonicalize_type(method.ty)
                });
                members.insert(format!(
                    "static_method:{}:{}",
                    escape(&method.name),
                    method_sig
                ));
            }

            if let Some(parent) = class_ty.extends {
                members.insert(format!("extends:{}", this.canonicalize_type(parent)));
            }

            if !class_ty.implements.is_empty() {
                let mut impls = class_ty
                    .implements
                    .iter()
                    .map(|iface| this.canonicalize_type(*iface))
                    .collect::<Vec<_>>();
                impls.sort_unstable();
                impls.dedup();
                members.insert(format!("implements:[{}]", impls.join(",")));
            }

            format!(
                "class_pub({})",
                members.into_iter().collect::<Vec<_>>().join(",")
            )
        })
    }

    fn canonicalize_interface(&mut self, iface: &super::ty::InterfaceType) -> String {
        self.with_alpha_params(&iface.type_params, |this| {
            let mut members = BTreeSet::new();

            for property in &iface.properties {
                if property.visibility != Visibility::Public {
                    continue;
                }
                let readonly = if property.readonly { "ro" } else { "rw" };
                let optional = if property.optional { "opt" } else { "req" };
                members.insert(format!(
                    "prop:{}:{}:{}:{}",
                    escape(&property.name),
                    readonly,
                    optional,
                    this.canonicalize_type(property.ty)
                ));
            }

            for method in &iface.methods {
                if method.visibility != Visibility::Public {
                    continue;
                }
                let method_sig = this.with_alpha_params(&method.type_params, |this| {
                    this.canonicalize_type(method.ty)
                });
                members.insert(format!("method:{}:{}", escape(&method.name), method_sig));
            }

            if !iface.extends.is_empty() {
                let mut exts = iface
                    .extends
                    .iter()
                    .map(|extended| this.canonicalize_type(*extended))
                    .collect::<Vec<_>>();
                exts.sort_unstable();
                exts.dedup();
                members.insert(format!("extends:[{}]", exts.join(",")));
            }

            for sig in &iface.call_signatures {
                members.insert(format!("call:{}", this.canonicalize_type(*sig)));
            }

            for sig in &iface.construct_signatures {
                members.insert(format!("ctor:{}", this.canonicalize_type(*sig)));
            }

            format!(
                "interface_pub({})",
                members.into_iter().collect::<Vec<_>>().join(",")
            )
        })
    }

    fn canonicalize_type_var(&mut self, tv: &super::ty::TypeVar) -> String {
        let canonical_name = self.resolve_or_bind_type_var(&tv.name);
        let constraint = tv
            .constraint
            .map(|constraint| self.canonicalize_type(constraint))
            .unwrap_or_else(|| "_".to_string());
        let default = tv
            .default
            .map(|default| self.canonicalize_type(default))
            .unwrap_or_else(|| "_".to_string());
        format!(
            "tv({};extends={};default={})",
            canonical_name, constraint, default
        )
    }

    fn with_alpha_params<F>(&mut self, params: &[String], f: F) -> String
    where
        F: FnOnce(&mut Self) -> String,
    {
        let mut scope = HashMap::new();
        for param in params {
            let canonical_name = format!("T{}", self.next_type_var);
            self.next_type_var += 1;
            scope.insert(param.clone(), canonical_name);
        }
        self.type_var_scopes.push(scope);
        let rendered = f(self);
        self.type_var_scopes.pop();
        rendered
    }

    fn resolve_or_bind_type_var(&mut self, name: &str) -> String {
        for scope in self.type_var_scopes.iter().rev() {
            if let Some(mapped) = scope.get(name) {
                return mapped.clone();
            }
        }

        let canonical_name = format!("T{}", self.next_type_var);
        self.next_type_var += 1;
        if let Some(scope) = self.type_var_scopes.last_mut() {
            scope.insert(name.to_string(), canonical_name.clone());
        } else {
            let mut scope = HashMap::new();
            scope.insert(name.to_string(), canonical_name.clone());
            self.type_var_scopes.push(scope);
        }
        canonical_name
    }
}

fn canonical_primitive(primitive: PrimitiveType) -> &'static str {
    match primitive {
        PrimitiveType::Number => "number",
        PrimitiveType::Int => "int",
        PrimitiveType::String => "string",
        PrimitiveType::Boolean => "boolean",
        PrimitiveType::Null => "null",
        PrimitiveType::Void => "void",
    }
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace(',', "\\,")
        .replace('|', "\\|")
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        out.push(ch);
    }
    if escaped {
        out.push('\\');
    }
    out
}

fn split_top_level<'a>(input: &'a str, delimiter: char) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth_paren = 0usize;
    let mut depth_bracket = 0usize;
    let mut depth_angle = 0usize;
    let mut escaped = false;

    for (idx, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' => {
                escaped = true;
            }
            '(' => depth_paren = depth_paren.saturating_add(1),
            ')' => depth_paren = depth_paren.saturating_sub(1),
            '[' => depth_bracket = depth_bracket.saturating_add(1),
            ']' => depth_bracket = depth_bracket.saturating_sub(1),
            '<' => depth_angle = depth_angle.saturating_add(1),
            '>' => depth_angle = depth_angle.saturating_sub(1),
            _ => {}
        }

        if ch == delimiter && depth_paren == 0 && depth_bracket == 0 && depth_angle == 0 {
            parts.push(input[start..idx].trim());
            start = idx + ch.len_utf8();
        }
    }

    if start <= input.len() {
        parts.push(input[start..].trim());
    }

    parts
}

fn strip_wrapped<'a>(value: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    value.strip_prefix(prefix)?.strip_suffix(suffix)
}

struct SignatureHydrator<'a> {
    type_ctx: &'a mut TypeContext,
    type_vars: HashMap<String, TypeId>,
    // Active recursion slots for @Rk markers while parsing canonical signatures.
    recursion_slots: Vec<Option<TypeId>>,
    next_placeholder_id: u64,
}

impl<'a> SignatureHydrator<'a> {
    fn new(type_ctx: &'a mut TypeContext) -> Self {
        Self {
            type_ctx,
            type_vars: HashMap::new(),
            recursion_slots: Vec::new(),
            next_placeholder_id: 0,
        }
    }

    fn parse_type(&mut self, raw: &str) -> Option<TypeId> {
        let slot_idx = self.recursion_slots.len();
        self.recursion_slots.push(None);

        let parsed = self.parse_type_inner(raw);
        let resolved = match (self.recursion_slots[slot_idx], parsed) {
            (Some(slot_ty), Some(parsed_ty)) => {
                if slot_ty != parsed_ty {
                    let new_ty = self.type_ctx.get(parsed_ty).cloned()?;
                    self.type_ctx.replace_type(slot_ty, new_ty);
                }
                slot_ty
            }
            (None, Some(parsed_ty)) => {
                self.recursion_slots[slot_idx] = Some(parsed_ty);
                parsed_ty
            }
            (Some(slot_ty), None) => slot_ty,
            (None, None) => {
                self.recursion_slots.pop();
                return None;
            }
        };
        self.recursion_slots.pop();
        Some(resolved)
    }

    fn parse_type_inner(&mut self, raw: &str) -> Option<TypeId> {
        let value = raw.trim();
        if value.is_empty() {
            return None;
        }

        match value {
            "number" => return Some(self.type_ctx.number_type()),
            "int" => return Some(self.type_ctx.int_type()),
            "string" => return Some(self.type_ctx.string_type()),
            "boolean" => return Some(self.type_ctx.boolean_type()),
            "null" => return Some(self.type_ctx.null_type()),
            "void" => return Some(self.type_ctx.void_type()),
            "never" => return Some(self.type_ctx.never_type()),
            "unknown" => return Some(self.type_ctx.unknown_type()),
            "any" => return Some(self.type_ctx.any_type()),
            "js_object" => return Some(self.type_ctx.jsobject_type()),
            "Mutex" => return Some(self.type_ctx.mutex_type()),
            "RegExp" => return Some(self.type_ctx.regexp_type()),
            "Date" => return Some(self.type_ctx.date_type()),
            "Buffer" => return Some(self.type_ctx.buffer_type()),
            "json" => return Some(self.type_ctx.json_type()),
            _ => {}
        }

        if let Some(rest) = value.strip_prefix("@R") {
            let rec_idx = rest.parse::<usize>().ok()?;
            if rec_idx >= self.recursion_slots.len() {
                return None;
            }
            if let Some(existing) = self.recursion_slots[rec_idx] {
                return Some(existing);
            }
            let placeholder = self.make_recursive_placeholder(rec_idx);
            self.recursion_slots[rec_idx] = Some(placeholder);
            return Some(placeholder);
        }

        if let Some(inner) = strip_wrapped(value, "strlit(", ")") {
            return Some(self.type_ctx.string_literal(unescape(inner)));
        }

        if let Some(inner) = strip_wrapped(value, "numlit(", ")") {
            let bits = u64::from_str_radix(inner, 16).ok()?;
            return Some(self.type_ctx.number_literal(f64::from_bits(bits)));
        }

        if let Some(inner) = strip_wrapped(value, "boollit(", ")") {
            let parsed = inner == "1" || inner.eq_ignore_ascii_case("true");
            return Some(self.type_ctx.boolean_literal(parsed));
        }

        if let Some(inner) = strip_wrapped(value, "arr(", ")") {
            let element = self.parse_type(inner)?;
            return Some(self.type_ctx.array_type(element));
        }

        if let Some(inner) = strip_wrapped(value, "Promise<", ">") {
            let result = self.parse_type(inner)?;
            return Some(self.type_ctx.task_type(result));
        }

        if let Some(inner) = strip_wrapped(value, "Channel<", ">") {
            let message = self.parse_type(inner)?;
            return Some(self.type_ctx.channel_type_with(message));
        }

        if let Some(inner) = strip_wrapped(value, "Map<", ">") {
            let parts = split_top_level(inner, ',');
            if parts.len() != 2 {
                return None;
            }
            let key = self.parse_type(&parts[0])?;
            let value = self.parse_type(&parts[1])?;
            return Some(self.type_ctx.map_type_with(key, value));
        }

        if let Some(inner) = strip_wrapped(value, "Set<", ">") {
            let element = self.parse_type(inner)?;
            return Some(self.type_ctx.set_type_with(element));
        }

        if let Some(inner) = strip_wrapped(value, "tuple(", ")") {
            if inner.trim().is_empty() {
                return Some(self.type_ctx.tuple_type(Vec::new()));
            }
            let elements = split_top_level(inner, ',')
                .into_iter()
                .filter(|part| !part.is_empty())
                .filter_map(|part| self.parse_type(&part))
                .collect::<Vec<_>>();
            return Some(self.type_ctx.tuple_type(elements));
        }

        if let Some(inner) = strip_wrapped(value, "union(", ")") {
            if inner.trim().is_empty() {
                return Some(self.type_ctx.never_type());
            }
            let members = split_top_level(inner, '|')
                .into_iter()
                .filter(|part| !part.is_empty())
                .filter_map(|part| self.parse_type(&part))
                .collect::<Vec<_>>();
            return Some(self.type_ctx.union_type(members));
        }

        if let Some(inner) = strip_wrapped(value, "fn(", ")") {
            return self.parse_function(inner);
        }

        if let Some(inner) = strip_wrapped(value, "obj(", ")") {
            return self.parse_object(inner);
        }

        if let Some(inner) = strip_wrapped(value, "class_pub(", ")") {
            return self.parse_class(inner);
        }

        if let Some(inner) = strip_wrapped(value, "interface_pub(", ")") {
            return self.parse_interface(inner);
        }

        if let Some(inner) = strip_wrapped(value, "generic(", ")") {
            return self.parse_generic(inner);
        }

        if let Some(inner) = strip_wrapped(value, "alias(", ")") {
            return self.parse_type(inner);
        }

        if let Some(inner) = strip_wrapped(value, "ref(", ")") {
            return self.parse_reference(inner);
        }

        if let Some(inner) = strip_wrapped(value, "keyof(", ")") {
            let target = self.parse_type(inner)?;
            return Some(self.type_ctx.keyof_type(target));
        }

        if let Some(inner) = strip_wrapped(value, "index(", ")") {
            let parts = split_top_level(inner, ',');
            if parts.len() != 2 {
                return None;
            }
            let object = self.parse_type(&parts[0])?;
            let index = self.parse_type(&parts[1])?;
            return Some(self.type_ctx.indexed_access_type(object, index));
        }

        if let Some(inner) = strip_wrapped(value, "tv(", ")") {
            return self.parse_type_var(inner);
        }

        let name = unescape(value);
        self.type_ctx.lookup_named_type(&name)
    }

    fn make_recursive_placeholder(&mut self, rec_idx: usize) -> TypeId {
        let unique = self.next_placeholder_id;
        self.next_placeholder_id = self.next_placeholder_id.saturating_add(1);
        self.type_ctx
            .intern(Type::Reference(super::ty::TypeReference {
                name: format!("__sig_rec_{rec_idx}_{unique}"),
                type_args: None,
            }))
    }

    fn parse_function(&mut self, inner: &str) -> Option<TypeId> {
        let mut min_params = 0usize;
        let mut params = Vec::<TypeId>::new();
        let mut rest = None;
        let mut return_type = self.type_ctx.unknown_type();

        for part in split_top_level(inner, ',') {
            if let Some(value) = part.strip_prefix("min=") {
                min_params = value.parse::<usize>().ok()?;
                continue;
            }
            if let Some(value) = part.strip_prefix("params=") {
                let body = strip_wrapped(value, "[", "]")?;
                if !body.trim().is_empty() {
                    for param in split_top_level(body, ',') {
                        params.push(self.parse_type(&param)?);
                    }
                }
                continue;
            }
            if let Some(value) = part.strip_prefix("rest=") {
                if value != "_" {
                    rest = Some(self.parse_type(value)?);
                }
                continue;
            }
            if let Some(value) = part.strip_prefix("ret=") {
                return_type = self.parse_type(value)?;
            }
        }

        Some(
            self.type_ctx
                .function_type_with_rest(params, return_type, false, min_params, rest),
        )
    }

    fn parse_object(&mut self, inner: &str) -> Option<TypeId> {
        let mut properties = Vec::new();
        let mut index_signature = None::<(String, TypeId)>;
        let mut call_signatures = Vec::new();
        let mut construct_signatures = Vec::new();

        if !inner.trim().is_empty() {
            for entry in split_top_level(inner, ',') {
                let fields = split_top_level(&entry, ':');
                let Some(kind) = fields.first().copied() else {
                    continue;
                };

                match kind {
                    "prop" if fields.len() >= 5 => {
                        let name = unescape(&fields[1]);
                        let readonly = fields[2] == "ro";
                        let optional = fields[3] == "opt";
                        let ty = self.parse_type(&fields[4])?;
                        properties.push(super::ty::PropertySignature {
                            name,
                            ty,
                            optional,
                            readonly,
                            visibility: Visibility::Public,
                        });
                    }
                    "index" if fields.len() >= 3 => {
                        let key = unescape(&fields[1]);
                        let ty = self.parse_type(&fields[2])?;
                        index_signature = Some((key, ty));
                    }
                    "call" if fields.len() >= 2 => {
                        let ty = self.parse_type(&fields[1])?;
                        call_signatures.push(ty);
                    }
                    "ctor" if fields.len() >= 2 => {
                        let ty = self.parse_type(&fields[1])?;
                        construct_signatures.push(ty);
                    }
                    _ => {}
                }
            }
        }

        Some(self.type_ctx.intern(Type::Object(super::ty::ObjectType {
            properties,
            index_signature,
            call_signatures,
            construct_signatures,
        })))
    }

    fn parse_class(&mut self, inner: &str) -> Option<TypeId> {
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut static_properties = Vec::new();
        let mut static_methods = Vec::new();
        let mut extends = None::<TypeId>;
        let mut implements = Vec::new();

        if !inner.trim().is_empty() {
            for entry in split_top_level(inner, ',') {
                let fields = split_top_level(&entry, ':');
                let Some(kind) = fields.first().copied() else {
                    continue;
                };

                match kind {
                    "inst_prop" | "static_prop" if fields.len() >= 5 => {
                        let name = unescape(&fields[1]);
                        let readonly = fields[2] == "ro";
                        let optional = fields[3] == "opt";
                        let ty = self.parse_type(&fields[4])?;
                        let prop = super::ty::PropertySignature {
                            name,
                            ty,
                            optional,
                            readonly,
                            visibility: Visibility::Public,
                        };
                        if kind == "inst_prop" {
                            properties.push(prop);
                        } else {
                            static_properties.push(prop);
                        }
                    }
                    "inst_method" | "static_method" if fields.len() >= 3 => {
                        let name = unescape(&fields[1]);
                        let ty = self.parse_type(&fields[2])?;
                        let method = super::ty::MethodSignature {
                            name,
                            ty,
                            type_params: Vec::new(),
                            visibility: Visibility::Public,
                        };
                        if kind == "inst_method" {
                            methods.push(method);
                        } else {
                            static_methods.push(method);
                        }
                    }
                    "extends" if fields.len() >= 2 => {
                        extends = Some(self.parse_type(&fields[1])?);
                    }
                    "implements" if fields.len() >= 2 => {
                        let body = strip_wrapped(&fields[1], "[", "]").unwrap_or("");
                        if !body.trim().is_empty() {
                            for item in split_top_level(body, ',') {
                                if let Some(interface_id) = self.parse_type(&item) {
                                    implements.push(interface_id);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if self.matches_buffer_surface(&properties, &methods) {
            return Some(self.type_ctx.buffer_type());
        }

        let class_name = format!("__imported_class_{:016x}", signature_hash(inner));
        Some(self.type_ctx.intern(Type::Class(super::ty::ClassType {
            name: class_name,
            type_params: Vec::new(),
            properties,
            methods,
            static_properties,
            static_methods,
            extends,
            implements,
            is_abstract: false,
        })))
    }

    fn matches_buffer_surface(
        &self,
        properties: &[super::ty::PropertySignature],
        methods: &[super::ty::MethodSignature],
    ) -> bool {
        let prop_names = properties
            .iter()
            .filter(|prop| prop.visibility == Visibility::Public)
            .map(|prop| prop.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let method_names = methods
            .iter()
            .filter(|method| method.visibility == Visibility::Public)
            .map(|method| method.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();

        prop_names.len() == 1
            && prop_names.contains("length")
            && method_names.contains("getByte")
            && method_names.contains("setByte")
            && method_names.contains("getInt32")
            && method_names.contains("setInt32")
            && method_names.contains("getFloat64")
            && method_names.contains("setFloat64")
            && method_names.contains("slice")
            && method_names.contains("copy")
            && method_names.contains("toString")
    }

    fn parse_interface(&mut self, inner: &str) -> Option<TypeId> {
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut call_signatures = Vec::new();
        let mut construct_signatures = Vec::new();
        let mut extends = Vec::new();

        if !inner.trim().is_empty() {
            for entry in split_top_level(inner, ',') {
                let fields = split_top_level(&entry, ':');
                let Some(kind) = fields.first().copied() else {
                    continue;
                };

                match kind {
                    "prop" if fields.len() >= 5 => {
                        let name = unescape(&fields[1]);
                        let readonly = fields[2] == "ro";
                        let optional = fields[3] == "opt";
                        let ty = self.parse_type(&fields[4])?;
                        properties.push(super::ty::PropertySignature {
                            name,
                            ty,
                            optional,
                            readonly,
                            visibility: Visibility::Public,
                        });
                    }
                    "method" if fields.len() >= 3 => {
                        let name = unescape(&fields[1]);
                        let ty = self.parse_type(&fields[2])?;
                        methods.push(super::ty::MethodSignature {
                            name,
                            ty,
                            type_params: Vec::new(),
                            visibility: Visibility::Public,
                        });
                    }
                    "extends" if fields.len() >= 2 => {
                        let body = strip_wrapped(&fields[1], "[", "]").unwrap_or("");
                        if !body.trim().is_empty() {
                            for item in split_top_level(body, ',') {
                                if let Some(interface_id) = self.parse_type(&item) {
                                    extends.push(interface_id);
                                }
                            }
                        }
                    }
                    "call" if fields.len() >= 2 => {
                        let ty = self.parse_type(&fields[1])?;
                        call_signatures.push(ty);
                    }
                    "ctor" if fields.len() >= 2 => {
                        let ty = self.parse_type(&fields[1])?;
                        construct_signatures.push(ty);
                    }
                    _ => {}
                }
            }
        }

        let interface_name = format!("__imported_interface_{:016x}", signature_hash(inner));
        Some(
            self.type_ctx
                .intern(Type::Interface(super::ty::InterfaceType {
                    name: interface_name,
                    type_params: Vec::new(),
                    properties,
                    methods,
                    call_signatures,
                    construct_signatures,
                    extends,
                })),
        )
    }

    fn parse_generic(&mut self, inner: &str) -> Option<TypeId> {
        let parts = split_top_level(inner, ',');
        if parts.len() != 2 {
            return None;
        }
        let base = self.parse_type(&parts[0])?;
        let args_body = strip_wrapped(&parts[1], "[", "]").unwrap_or("");
        let mut type_args = Vec::new();
        if !args_body.trim().is_empty() {
            for part in split_top_level(args_body, ',') {
                type_args.push(self.parse_type(&part)?);
            }
        }
        Some(
            self.type_ctx
                .intern(Type::Generic(super::ty::GenericType { base, type_args })),
        )
    }

    fn parse_reference(&mut self, inner: &str) -> Option<TypeId> {
        let parts = split_top_level(inner, ',');
        if parts.is_empty() {
            return None;
        }

        let name = unescape(&parts[0]);
        let type_args = if parts.len() >= 2 {
            let args_body = strip_wrapped(&parts[1], "[", "]").unwrap_or("");
            let mut parsed_args = Vec::new();
            if !args_body.trim().is_empty() {
                for part in split_top_level(args_body, ',') {
                    parsed_args.push(self.parse_type(&part)?);
                }
            }
            if parsed_args.is_empty() {
                None
            } else {
                Some(parsed_args)
            }
        } else {
            None
        };

        if type_args.is_none() {
            if let Some(existing) = self.type_ctx.lookup_named_type(&name) {
                return Some(existing);
            }
        }

        Some(
            self.type_ctx
                .intern(Type::Reference(super::ty::TypeReference {
                    name,
                    type_args,
                })),
        )
    }

    fn parse_type_var(&mut self, inner: &str) -> Option<TypeId> {
        let parts = split_top_level(inner, ';');
        let raw_name = parts.first()?.trim();
        if raw_name.is_empty() {
            return None;
        }
        let canonical_name = raw_name.to_string();
        if let Some(existing) = self.type_vars.get(&canonical_name).copied() {
            return Some(existing);
        }

        let mut constraint = None;
        let mut default = None;
        for part in parts.into_iter().skip(1) {
            if let Some(value) = part.strip_prefix("extends=") {
                if value != "_" {
                    constraint = self.parse_type(value);
                }
                continue;
            }
            if let Some(value) = part.strip_prefix("default=") {
                if value != "_" {
                    default = self.parse_type(value);
                }
            }
        }

        let ty = self.type_ctx.intern(Type::TypeVar(super::ty::TypeVar {
            name: canonical_name.clone(),
            constraint,
            default,
        }));
        self.type_vars.insert(canonical_name, ty);
        Some(ty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_member_order_does_not_change_signature() {
        use crate::parser::ast::Visibility;
        use crate::parser::types::ty::PropertySignature;

        let mut a_ctx = TypeContext::new();
        let number = a_ctx.number_type();
        let string = a_ctx.string_type();
        let a = a_ctx.object_type(vec![
            PropertySignature {
                name: "b".to_string(),
                ty: number,
                optional: false,
                readonly: false,
                visibility: Visibility::Public,
            },
            PropertySignature {
                name: "a".to_string(),
                ty: string,
                optional: false,
                readonly: false,
                visibility: Visibility::Public,
            },
        ]);

        let mut b_ctx = TypeContext::new();
        let number = b_ctx.number_type();
        let string = b_ctx.string_type();
        let b = b_ctx.object_type(vec![
            PropertySignature {
                name: "a".to_string(),
                ty: string,
                optional: false,
                readonly: false,
                visibility: Visibility::Public,
            },
            PropertySignature {
                name: "b".to_string(),
                ty: number,
                optional: false,
                readonly: false,
                visibility: Visibility::Public,
            },
        ]);

        let a_sig = canonical_type_signature(a, &a_ctx);
        let b_sig = canonical_type_signature(b, &b_ctx);
        assert_eq!(a_sig.canonical, b_sig.canonical);
        assert_eq!(a_sig.hash, b_sig.hash);
    }

    #[test]
    fn union_members_are_flattened_sorted_and_deduped() {
        let mut ctx = TypeContext::new();
        let number = ctx.number_type();
        let string = ctx.string_type();
        let boolean = ctx.boolean_type();
        let nested = ctx.union_type(vec![number, string]);
        let union = ctx.union_type(vec![boolean, nested, number]);

        let sig = canonical_type_signature(union, &ctx);
        assert!(sig.canonical.contains("union("));
        assert_eq!(sig.canonical.matches("number").count(), 1);
    }

    #[test]
    fn async_function_normalizes_to_promise_shape() {
        let mut ctx = TypeContext::new();
        let number = ctx.number_type();
        let async_fn = ctx.function_type(vec![number], number, true);
        let promise_number = ctx.task_type(number);
        let plain_fn = ctx.function_type(vec![number], promise_number, false);

        let a = canonical_type_signature(async_fn, &ctx);
        let b = canonical_type_signature(plain_fn, &ctx);
        assert_eq!(a.hash, b.hash);
        assert_eq!(a.canonical, b.canonical);
    }

    #[test]
    fn hydrate_function_signature_restores_callable_shape() {
        let mut ctx = TypeContext::new();
        let sig = "fn(min=1,params=[number,string],rest=arr(boolean),ret=Promise<number>)";
        let hydrated = hydrate_type_from_canonical_signature(sig, &mut ctx);

        let Some(Type::Function(function)) = ctx.get(hydrated) else {
            panic!("expected hydrated function type");
        };

        assert_eq!(function.min_params, 1);
        assert_eq!(function.params.len(), 2);
        assert!(function.rest_param.is_some());
        let hydrated_sig = canonical_type_signature(hydrated, &ctx);
        assert_eq!(hydrated_sig.canonical, sig);
    }

    #[test]
    fn hydrate_class_signature_restores_public_members() {
        let mut ctx = TypeContext::new();
        let sig = "class_pub(inst_method:foo:fn(min=1,params=[number],rest=_,ret=number),inst_prop:x:rw:req:number,static_method:build:fn(min=0,params=[],rest=_,ret=class_pub(inst_prop:x:rw:req:number)))";
        let hydrated = hydrate_type_from_canonical_signature(sig, &mut ctx);

        let Some(Type::Class(class_ty)) = ctx.get(hydrated) else {
            panic!("expected hydrated class type");
        };

        assert!(class_ty.methods.iter().any(|method| method.name == "foo"));
        assert!(class_ty
            .properties
            .iter()
            .any(|property| property.name == "x"));
        assert!(class_ty
            .static_methods
            .iter()
            .any(|method| method.name == "build"));
    }

    #[test]
    fn class_names_do_not_affect_structural_signature_hash() {
        use crate::parser::types::ty::{ClassType, MethodSignature, PropertySignature};

        let mut left_ctx = TypeContext::new();
        let number = left_ctx.number_type();
        let left_get = left_ctx.function_type(vec![], number, false);
        let left_class = left_ctx.intern(Type::Class(ClassType {
            name: "Alpha".to_string(),
            type_params: vec![],
            properties: vec![PropertySignature {
                name: "value".to_string(),
                ty: number,
                optional: false,
                readonly: false,
                visibility: Visibility::Public,
            }],
            methods: vec![MethodSignature {
                name: "get".to_string(),
                ty: left_get,
                type_params: vec![],
                visibility: Visibility::Public,
            }],
            static_properties: vec![],
            static_methods: vec![],
            extends: None,
            implements: vec![],
            is_abstract: false,
        }));

        let mut right_ctx = TypeContext::new();
        let number = right_ctx.number_type();
        let right_get = right_ctx.function_type(vec![], number, false);
        let right_class = right_ctx.intern(Type::Class(ClassType {
            name: "Beta".to_string(),
            type_params: vec![],
            properties: vec![PropertySignature {
                name: "value".to_string(),
                ty: number,
                optional: false,
                readonly: false,
                visibility: Visibility::Public,
            }],
            methods: vec![MethodSignature {
                name: "get".to_string(),
                ty: right_get,
                type_params: vec![],
                visibility: Visibility::Public,
            }],
            static_properties: vec![],
            static_methods: vec![],
            extends: None,
            implements: vec![],
            is_abstract: false,
        }));

        let left = canonical_type_signature(left_class, &left_ctx);
        let right = canonical_type_signature(right_class, &right_ctx);
        assert_eq!(left.canonical, right.canonical);
        assert_eq!(left.hash, right.hash);
    }

    #[test]
    fn generic_alpha_equivalent_signatures_match() {
        let mut left_ctx = TypeContext::new();
        let left_t = left_ctx.type_variable("T");
        let left_fn = left_ctx.function_type(vec![left_t], left_t, false);

        let mut right_ctx = TypeContext::new();
        let right_u = right_ctx.type_variable("U");
        let right_fn = right_ctx.function_type(vec![right_u], right_u, false);

        let left = canonical_type_signature(left_fn, &left_ctx);
        let right = canonical_type_signature(right_fn, &right_ctx);
        assert_eq!(left.canonical, right.canonical);
        assert_eq!(left.hash, right.hash);
    }

    #[test]
    fn structural_assignable_accepts_object_width_subtype() {
        let expected = "obj(prop:a:rw:req:number,prop:b:rw:req:string)";
        let actual = "obj(prop:a:rw:req:number,prop:b:rw:req:string,prop:c:rw:req:string)";
        assert!(structural_signature_is_assignable(expected, actual));
    }

    #[test]
    fn structural_assignable_accepts_union_subset() {
        let expected = "union(number|string)";
        let actual = "number";
        assert!(structural_signature_is_assignable(expected, actual));
        assert!(!structural_signature_is_assignable(actual, expected));
    }

    #[test]
    fn structural_assignable_accepts_function_with_fewer_declared_args() {
        let expected = "fn(min=2,params=[number,number],rest=_,ret=number)";
        let actual = "fn(min=1,params=[number],rest=_,ret=number)";
        assert!(structural_signature_is_assignable(expected, actual));
    }
}
