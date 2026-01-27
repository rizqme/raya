//! Generic type system: instantiation, substitution, and unification
//!
//! Implements the generic type system for Raya, including:
//! - Type variable substitution
//! - Generic type instantiation
//! - Constraint checking
//! - Type unification

use super::context::TypeContext;
use super::error::TypeError;
use super::subtyping::SubtypingContext;
use super::ty::{Type, TypeId};
use rustc_hash::FxHashMap;

/// Context for generic type operations
#[derive(Debug)]
pub struct GenericContext<'a> {
    /// Type context for resolving types
    type_ctx: &'a mut TypeContext,

    /// Current type variable substitutions
    substitutions: FxHashMap<String, TypeId>,
}

impl<'a> GenericContext<'a> {
    /// Create a new generic context
    pub fn new(type_ctx: &'a mut TypeContext) -> Self {
        GenericContext {
            type_ctx,
            substitutions: FxHashMap::default(),
        }
    }

    /// Add a type variable substitution
    pub fn add_substitution(&mut self, name: String, ty: TypeId) {
        self.substitutions.insert(name, ty);
    }

    /// Get a type variable substitution
    pub fn get_substitution(&self, name: &str) -> Option<TypeId> {
        self.substitutions.get(name).copied()
    }

    /// Clear all substitutions
    pub fn clear_substitutions(&mut self) {
        self.substitutions.clear();
    }

    /// Apply substitutions to a type
    ///
    /// Replaces all type variables with their substitutions.
    pub fn apply_substitution(&mut self, ty: TypeId) -> Result<TypeId, TypeError> {
        // Clone the type data to avoid borrow checker issues
        let ty_data = self.type_ctx.get(ty).ok_or_else(|| TypeError::Generic {
            message: format!("Invalid type ID: {:?}", ty),
        })?.clone();

        match ty_data {
            Type::TypeVar(tv) => {
                // Check if we have a substitution for this type variable
                if let Some(substitution) = self.substitutions.get(&tv.name) {
                    // Recursively apply substitutions
                    return self.apply_substitution(*substitution);
                }
                Ok(ty)
            }

            Type::Array(arr) => {
                let elem = self.apply_substitution(arr.element)?;
                Ok(self.type_ctx.array_type(elem))
            }

            Type::Task(task) => {
                let result = self.apply_substitution(task.result)?;
                Ok(self.type_ctx.task_type(result))
            }

            Type::Tuple(tuple) => {
                let elem_ids = tuple.elements.clone();
                let mut elements = Vec::new();
                for elem in elem_ids {
                    elements.push(self.apply_substitution(elem)?);
                }
                Ok(self.type_ctx.tuple_type(elements))
            }

            Type::Function(func) => {
                let param_ids = func.params.clone();
                let return_id = func.return_type;
                let is_async = func.is_async;

                let mut params = Vec::new();
                for param in param_ids {
                    params.push(self.apply_substitution(param)?);
                }
                let return_type = self.apply_substitution(return_id)?;
                Ok(self.type_ctx.function_type(params, return_type, is_async))
            }

            Type::Union(union) => {
                let member_ids = union.members.clone();

                let mut members = Vec::new();
                for member in member_ids {
                    members.push(self.apply_substitution(member)?);
                }
                // Discriminant will be re-inferred for the substituted members
                Ok(self.type_ctx.union_type(members))
            }

            Type::Generic(gen) => {
                let base_id = gen.base;
                let arg_ids = gen.type_args.clone();

                let base = self.apply_substitution(base_id)?;
                let mut type_args = Vec::new();
                for arg in arg_ids {
                    type_args.push(self.apply_substitution(arg)?);
                }

                // Create new generic type with substituted arguments
                let gen_ty = Type::Generic(super::ty::GenericType { base, type_args });
                Ok(self.type_ctx.intern(gen_ty))
            }

            // Other types don't contain type variables
            _ => Ok(ty),
        }
    }

    /// Instantiate a generic type with concrete type arguments
    ///
    /// Example: `Array<T>` with `[number]` -> `Array<number>`
    pub fn instantiate(
        &mut self,
        generic_ty: TypeId,
        type_args: &[TypeId],
    ) -> Result<TypeId, TypeError> {
        let ty_data = self.type_ctx.get(generic_ty).ok_or_else(|| TypeError::Generic {
            message: format!("Invalid type ID: {:?}", generic_ty),
        })?;

        match ty_data {
            Type::TypeVar(tv) => {
                // Get type parameters from constraint
                if type_args.len() != 1 {
                    return Err(TypeError::InvalidTypeArgCount {
                        expected: 1,
                        actual: type_args.len(),
                    });
                }

                // Check constraint if present
                if let Some(constraint) = tv.constraint {
                    self.check_constraint(type_args[0], constraint)?;
                }

                Ok(type_args[0])
            }

            Type::Reference(ref_ty) => {
                // Type reference with type parameters
                if let Some(expected_params) = &ref_ty.type_args {
                    if expected_params.len() != type_args.len() {
                        return Err(TypeError::InvalidTypeArgCount {
                            expected: expected_params.len(),
                            actual: type_args.len(),
                        });
                    }
                }

                // Create new reference with concrete type arguments
                let new_ref = Type::Reference(super::ty::TypeReference {
                    name: ref_ty.name.clone(),
                    type_args: Some(type_args.to_vec()),
                });

                Ok(self.type_ctx.intern(new_ref))
            }

            Type::Generic(gen) => {
                // Already instantiated generic - check argument count
                if gen.type_args.len() != type_args.len() {
                    return Err(TypeError::InvalidTypeArgCount {
                        expected: gen.type_args.len(),
                        actual: type_args.len(),
                    });
                }

                let base_id = gen.base;

                // Apply substitution to each type argument
                let mut new_args = Vec::new();
                for &arg in type_args {
                    new_args.push(self.apply_substitution(arg)?);
                }

                let new_gen = Type::Generic(super::ty::GenericType {
                    base: base_id,
                    type_args: new_args,
                });

                Ok(self.type_ctx.intern(new_gen))
            }

            _ => Err(TypeError::Generic {
                message: format!("Type {:?} is not generic", ty_data),
            }),
        }
    }

    /// Check if a type satisfies a constraint
    fn check_constraint(&self, ty: TypeId, constraint: TypeId) -> Result<(), TypeError> {
        // Use subtyping to check constraint
        let mut sub_ctx = SubtypingContext::new(self.type_ctx);

        if sub_ctx.is_subtype(ty, constraint) {
            Ok(())
        } else {
            Err(TypeError::ConstraintViolation {
                constraint: format!(
                    "{} does not satisfy constraint {}",
                    self.type_ctx.display(ty),
                    self.type_ctx.display(constraint)
                ),
            })
        }
    }

    /// Unify two types, finding substitutions that make them equal
    ///
    /// Returns true if unification succeeds, updating substitutions.
    pub fn unify(&mut self, ty1: TypeId, ty2: TypeId) -> Result<bool, TypeError> {
        // If types are already equal, nothing to do
        if ty1 == ty2 {
            return Ok(true);
        }

        // Clone the types to avoid borrow checker issues
        let ty1_data = self.type_ctx.get(ty1).ok_or_else(|| TypeError::Generic {
            message: format!("Invalid type ID: {:?}", ty1),
        })?.clone();

        let ty2_data = self.type_ctx.get(ty2).ok_or_else(|| TypeError::Generic {
            message: format!("Invalid type ID: {:?}", ty2),
        })?.clone();

        match (&ty1_data, &ty2_data) {
            // Type variable unification
            (Type::TypeVar(tv), _) => {
                // Check if already substituted
                if let Some(sub) = self.substitutions.get(&tv.name) {
                    return self.unify(*sub, ty2);
                }

                // Check constraint
                if let Some(constraint) = tv.constraint {
                    self.check_constraint(ty2, constraint)?;
                }

                // Add substitution
                self.substitutions.insert(tv.name.clone(), ty2);
                Ok(true)
            }

            (_, Type::TypeVar(tv)) => {
                // Symmetric case
                if let Some(sub) = self.substitutions.get(&tv.name) {
                    return self.unify(ty1, *sub);
                }

                if let Some(constraint) = tv.constraint {
                    self.check_constraint(ty1, constraint)?;
                }

                self.substitutions.insert(tv.name.clone(), ty1);
                Ok(true)
            }

            // Array unification
            (Type::Array(a1), Type::Array(a2)) => self.unify(a1.element, a2.element),

            // Task unification
            (Type::Task(t1), Type::Task(t2)) => self.unify(t1.result, t2.result),

            // Tuple unification
            (Type::Tuple(t1), Type::Tuple(t2)) => {
                if t1.elements.len() != t2.elements.len() {
                    return Ok(false);
                }

                for (&e1, &e2) in t1.elements.iter().zip(&t2.elements) {
                    if !self.unify(e1, e2)? {
                        return Ok(false);
                    }
                }

                Ok(true)
            }

            // Function unification
            (Type::Function(f1), Type::Function(f2)) => {
                if f1.params.len() != f2.params.len() || f1.is_async != f2.is_async {
                    return Ok(false);
                }

                // Unify parameters
                for (&p1, &p2) in f1.params.iter().zip(&f2.params) {
                    if !self.unify(p1, p2)? {
                        return Ok(false);
                    }
                }

                // Unify return type
                self.unify(f1.return_type, f2.return_type)
            }

            // Union unification
            (Type::Union(u1), Type::Union(u2)) => {
                if u1.members.len() != u2.members.len() {
                    return Ok(false);
                }

                // Simple approach: check if all members can be unified
                for (&m1, &m2) in u1.members.iter().zip(&u2.members) {
                    if !self.unify(m1, m2)? {
                        return Ok(false);
                    }
                }

                Ok(true)
            }

            // Generic unification
            (Type::Generic(g1), Type::Generic(g2)) => {
                if g1.base != g2.base || g1.type_args.len() != g2.type_args.len() {
                    return Ok(false);
                }

                for (&a1, &a2) in g1.type_args.iter().zip(&g2.type_args) {
                    if !self.unify(a1, a2)? {
                        return Ok(false);
                    }
                }

                Ok(true)
            }

            // Primitive types must be equal (already checked at top)
            (Type::Primitive(p1), Type::Primitive(p2)) => Ok(p1 == p2),

            // Reference types
            (Type::Reference(r1), Type::Reference(r2)) => {
                if r1.name != r2.name {
                    return Ok(false);
                }

                match (&r1.type_args, &r2.type_args) {
                    (Some(args1), Some(args2)) => {
                        if args1.len() != args2.len() {
                            return Ok(false);
                        }

                        for (&a1, &a2) in args1.iter().zip(args2) {
                            if !self.unify(a1, a2)? {
                                return Ok(false);
                            }
                        }

                        Ok(true)
                    }
                    (None, None) => Ok(true),
                    _ => Ok(false),
                }
            }

            // Different types cannot be unified
            _ => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ty::TypeVar;

    #[test]
    fn test_type_var_substitution() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();

        // Create type variable T
        let t_var = ctx.intern(Type::TypeVar(TypeVar {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }));

        let mut gen_ctx = GenericContext::new(&mut ctx);
        gen_ctx.add_substitution("T".to_string(), num);

        let result = gen_ctx.apply_substitution(t_var).unwrap();
        assert_eq!(result, num);
    }

    #[test]
    fn test_array_substitution() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();

        // Create Array<T>
        let t_var = ctx.intern(Type::TypeVar(TypeVar {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }));
        let array_t = ctx.array_type(t_var);

        let mut gen_ctx = GenericContext::new(&mut ctx);
        gen_ctx.add_substitution("T".to_string(), num);

        let result = gen_ctx.apply_substitution(array_t).unwrap();

        // Should be Array<number>
        match ctx.get(result) {
            Some(Type::Array(arr)) => {
                assert_eq!(arr.element, num);
            }
            _ => panic!("Expected array type"),
        }
    }

    #[test]
    fn test_function_substitution() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();

        // Create (T) => T
        let t_var = ctx.intern(Type::TypeVar(TypeVar {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }));
        let func = ctx.function_type(vec![t_var], t_var, false);

        let mut gen_ctx = GenericContext::new(&mut ctx);
        gen_ctx.add_substitution("T".to_string(), num);

        let result = gen_ctx.apply_substitution(func).unwrap();

        // Should be (number) => number
        match ctx.get(result) {
            Some(Type::Function(f)) => {
                assert_eq!(f.params.len(), 1);
                assert_eq!(f.params[0], num);
                assert_eq!(f.return_type, num);
            }
            _ => panic!("Expected function type"),
        }
    }

    #[test]
    fn test_constraint_checking() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str_ty = ctx.string_type();

        // Create T extends number
        let t_var = ctx.intern(Type::TypeVar(TypeVar {
            name: "T".to_string(),
            constraint: Some(num),
            default: None,
        }));

        let mut gen_ctx = GenericContext::new(&mut ctx);

        // number satisfies constraint
        assert!(gen_ctx.check_constraint(num, num).is_ok());

        // string does not satisfy constraint
        assert!(gen_ctx.check_constraint(str_ty, num).is_err());
    }

    #[test]
    fn test_unify_primitives() {
        let mut ctx = TypeContext::new();
        let num1 = ctx.number_type();
        let num2 = ctx.number_type();
        let str_ty = ctx.string_type();

        let mut gen_ctx = GenericContext::new(&mut ctx);

        // Same primitive types unify
        assert!(gen_ctx.unify(num1, num2).unwrap());

        // Different primitive types don't unify
        assert!(!gen_ctx.unify(num1, str_ty).unwrap());
    }

    #[test]
    fn test_unify_type_var() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();

        let t_var = ctx.intern(Type::TypeVar(TypeVar {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }));

        let mut gen_ctx = GenericContext::new(&mut ctx);

        // Unifying T with number should add substitution
        assert!(gen_ctx.unify(t_var, num).unwrap());
        assert_eq!(gen_ctx.get_substitution("T"), Some(num));
    }

    #[test]
    fn test_unify_arrays() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();

        let t_var = ctx.intern(Type::TypeVar(TypeVar {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }));

        let array_t = ctx.array_type(t_var);
        let array_num = ctx.array_type(num);

        let mut gen_ctx = GenericContext::new(&mut ctx);

        // T[] should unify with number[]
        assert!(gen_ctx.unify(array_t, array_num).unwrap());
        assert_eq!(gen_ctx.get_substitution("T"), Some(num));
    }

    #[test]
    fn test_unify_functions() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();

        let t_var = ctx.intern(Type::TypeVar(TypeVar {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }));

        // (T) => T
        let func1 = ctx.function_type(vec![t_var], t_var, false);

        // (number) => number
        let func2 = ctx.function_type(vec![num], num, false);

        let mut gen_ctx = GenericContext::new(&mut ctx);

        // Should unify with T = number
        assert!(gen_ctx.unify(func1, func2).unwrap());
        assert_eq!(gen_ctx.get_substitution("T"), Some(num));
    }

    #[test]
    fn test_unify_tuples() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str_ty = ctx.string_type();

        let t_var = ctx.intern(Type::TypeVar(TypeVar {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }));

        // [T, string]
        let tuple1 = ctx.tuple_type(vec![t_var, str_ty]);

        // [number, string]
        let tuple2 = ctx.tuple_type(vec![num, str_ty]);

        let mut gen_ctx = GenericContext::new(&mut ctx);

        // Should unify with T = number
        assert!(gen_ctx.unify(tuple1, tuple2).unwrap());
        assert_eq!(gen_ctx.get_substitution("T"), Some(num));
    }

    #[test]
    fn test_unify_different_lengths_fails() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str_ty = ctx.string_type();

        let tuple1 = ctx.tuple_type(vec![num]);
        let tuple2 = ctx.tuple_type(vec![num, str_ty]);

        let mut gen_ctx = GenericContext::new(&mut ctx);

        // Different length tuples don't unify
        assert!(!gen_ctx.unify(tuple1, tuple2).unwrap());
    }
}
