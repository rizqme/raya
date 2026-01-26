//! AST visitor pattern for traversing the syntax tree
//!
//! This module provides a visitor trait for walking the AST. Visitors can be used
//! for analysis, transformation, code generation, and other tree-walking operations.
//!
//! # Example
//!
//! ```rust
//! use raya_parser::ast::*;
//!
//! struct CountIdentifiers {
//!     count: usize,
//! }
//!
//! impl Visitor for CountIdentifiers {
//!     fn visit_identifier(&mut self, _id: &Identifier) {
//!         self.count += 1;
//!         // Identifier is a leaf node - no further traversal needed
//!     }
//! }
//! ```

use super::*;

/// AST visitor trait
///
/// Implement this trait to traverse the AST. Each visit method has a default
/// implementation that calls the corresponding walk function.
pub trait Visitor: Sized {
    // Top-level
    fn visit_module(&mut self, module: &Module) {
        walk_module(self, module);
    }

    // Statements
    fn visit_statement(&mut self, stmt: &Statement) {
        walk_statement(self, stmt);
    }

    fn visit_variable_decl(&mut self, decl: &VariableDecl) {
        walk_variable_decl(self, decl);
    }

    fn visit_function_decl(&mut self, decl: &FunctionDecl) {
        walk_function_decl(self, decl);
    }

    fn visit_class_decl(&mut self, decl: &ClassDecl) {
        walk_class_decl(self, decl);
    }

    fn visit_type_alias_decl(&mut self, decl: &TypeAliasDecl) {
        walk_type_alias_decl(self, decl);
    }

    fn visit_decorator(&mut self, decorator: &Decorator) {
        walk_decorator(self, decorator);
    }

    fn visit_import_decl(&mut self, decl: &ImportDecl) {
        walk_import_decl(self, decl);
    }

    fn visit_export_decl(&mut self, decl: &ExportDecl) {
        walk_export_decl(self, decl);
    }

    fn visit_if_statement(&mut self, stmt: &IfStatement) {
        walk_if_statement(self, stmt);
    }

    fn visit_switch_statement(&mut self, stmt: &SwitchStatement) {
        walk_switch_statement(self, stmt);
    }

    fn visit_while_statement(&mut self, stmt: &WhileStatement) {
        walk_while_statement(self, stmt);
    }

    fn visit_for_statement(&mut self, stmt: &ForStatement) {
        walk_for_statement(self, stmt);
    }

    fn visit_block_statement(&mut self, stmt: &BlockStatement) {
        walk_block_statement(self, stmt);
    }

    // Expressions
    fn visit_expression(&mut self, expr: &Expression) {
        walk_expression(self, expr);
    }

    fn visit_binary_expression(&mut self, expr: &BinaryExpression) {
        walk_binary_expression(self, expr);
    }

    fn visit_logical_expression(&mut self, expr: &LogicalExpression) {
        walk_logical_expression(self, expr);
    }

    fn visit_call_expression(&mut self, expr: &CallExpression) {
        walk_call_expression(self, expr);
    }

    fn visit_member_expression(&mut self, expr: &MemberExpression) {
        walk_member_expression(self, expr);
    }

    fn visit_array_expression(&mut self, expr: &ArrayExpression) {
        walk_array_expression(self, expr);
    }

    fn visit_object_expression(&mut self, expr: &ObjectExpression) {
        walk_object_expression(self, expr);
    }

    fn visit_arrow_function(&mut self, func: &ArrowFunction) {
        walk_arrow_function(self, func);
    }

    // JSX
    fn visit_jsx_element(&mut self, elem: &JsxElement) {
        walk_jsx_element(self, elem);
    }

    fn visit_jsx_fragment(&mut self, frag: &JsxFragment) {
        walk_jsx_fragment(self, frag);
    }

    // Types
    fn visit_type_annotation(&mut self, ty: &TypeAnnotation) {
        walk_type_annotation(self, ty);
    }

    fn visit_union_type(&mut self, union: &UnionType) {
        walk_union_type(self, union);
    }

    fn visit_function_type(&mut self, func: &FunctionType) {
        walk_function_type(self, func);
    }

    fn visit_object_type(&mut self, obj: &ObjectType) {
        walk_object_type(self, obj);
    }

    // Common
    fn visit_identifier(&mut self, _id: &Identifier) {
        // Leaf node - no traversal needed
    }

    fn visit_pattern(&mut self, pattern: &Pattern) {
        walk_pattern(self, pattern);
    }
}

// ============================================================================
// Walk Functions - Default Traversal Implementations
// ============================================================================

pub fn walk_module<V: Visitor>(visitor: &mut V, module: &Module) {
    for stmt in &module.statements {
        visitor.visit_statement(stmt);
    }
}

pub fn walk_statement<V: Visitor>(visitor: &mut V, stmt: &Statement) {
    match stmt {
        Statement::VariableDecl(decl) => visitor.visit_variable_decl(decl),
        Statement::FunctionDecl(decl) => visitor.visit_function_decl(decl),
        Statement::ClassDecl(decl) => visitor.visit_class_decl(decl),
        Statement::TypeAliasDecl(decl) => visitor.visit_type_alias_decl(decl),
        Statement::ImportDecl(decl) => visitor.visit_import_decl(decl),
        Statement::ExportDecl(decl) => visitor.visit_export_decl(decl),
        Statement::Expression(stmt) => visitor.visit_expression(&stmt.expression),
        Statement::If(stmt) => visitor.visit_if_statement(stmt),
        Statement::Switch(stmt) => visitor.visit_switch_statement(stmt),
        Statement::While(stmt) => visitor.visit_while_statement(stmt),
        Statement::DoWhile(stmt) => {
            visitor.visit_statement(&stmt.body);
            visitor.visit_expression(&stmt.condition);
        }
        Statement::For(stmt) => visitor.visit_for_statement(stmt),
        Statement::Break(_) | Statement::Continue(_) => {}
        Statement::Return(stmt) => {
            if let Some(value) = &stmt.value {
                visitor.visit_expression(value);
            }
        }
        Statement::Throw(stmt) => visitor.visit_expression(&stmt.value),
        Statement::Try(stmt) => {
            visitor.visit_block_statement(&stmt.body);
            if let Some(catch) = &stmt.catch_clause {
                if let Some(param) = &catch.param {
                    visitor.visit_pattern(param);
                }
                visitor.visit_block_statement(&catch.body);
            }
            if let Some(finally) = &stmt.finally_clause {
                visitor.visit_block_statement(finally);
            }
        }
        Statement::Block(stmt) => visitor.visit_block_statement(stmt),
        Statement::Empty(_) => {}
    }
}

pub fn walk_variable_decl<V: Visitor>(visitor: &mut V, decl: &VariableDecl) {
    visitor.visit_pattern(&decl.pattern);
    if let Some(type_ann) = &decl.type_annotation {
        visitor.visit_type_annotation(type_ann);
    }
    if let Some(init) = &decl.initializer {
        visitor.visit_expression(init);
    }
}

pub fn walk_function_decl<V: Visitor>(visitor: &mut V, decl: &FunctionDecl) {
    visitor.visit_identifier(&decl.name);
    if let Some(type_params) = &decl.type_params {
        for param in type_params {
            visitor.visit_identifier(&param.name);
            if let Some(constraint) = &param.constraint {
                visitor.visit_type_annotation(constraint);
            }
        }
    }
    for param in &decl.params {
        // Visit parameter decorators
        for decorator in &param.decorators {
            visitor.visit_decorator(decorator);
        }
        visitor.visit_pattern(&param.pattern);
        if let Some(type_ann) = &param.type_annotation {
            visitor.visit_type_annotation(type_ann);
        }
    }
    if let Some(return_type) = &decl.return_type {
        visitor.visit_type_annotation(return_type);
    }
    visitor.visit_block_statement(&decl.body);
}

pub fn walk_class_decl<V: Visitor>(visitor: &mut V, decl: &ClassDecl) {
    // Visit decorators
    for decorator in &decl.decorators {
        visitor.visit_decorator(decorator);
    }

    visitor.visit_identifier(&decl.name);
    if let Some(extends) = &decl.extends {
        visitor.visit_type_annotation(extends);
    }
    for impl_type in &decl.implements {
        visitor.visit_type_annotation(impl_type);
    }
    for member in &decl.members {
        match member {
            ClassMember::Field(field) => {
                // Visit field decorators
                for decorator in &field.decorators {
                    visitor.visit_decorator(decorator);
                }
                visitor.visit_identifier(&field.name);
                if let Some(type_ann) = &field.type_annotation {
                    visitor.visit_type_annotation(type_ann);
                }
                if let Some(init) = &field.initializer {
                    visitor.visit_expression(init);
                }
            }
            ClassMember::Method(method) => {
                // Visit method decorators
                for decorator in &method.decorators {
                    visitor.visit_decorator(decorator);
                }
                visitor.visit_identifier(&method.name);
                for param in &method.params {
                    // Visit parameter decorators
                    for decorator in &param.decorators {
                        visitor.visit_decorator(decorator);
                    }
                    visitor.visit_pattern(&param.pattern);
                    if let Some(type_ann) = &param.type_annotation {
                        visitor.visit_type_annotation(type_ann);
                    }
                }
                if let Some(return_type) = &method.return_type {
                    visitor.visit_type_annotation(return_type);
                }
                // Body is None for abstract methods
                if let Some(body) = &method.body {
                    visitor.visit_block_statement(body);
                }
            }
            ClassMember::Constructor(ctor) => {
                for param in &ctor.params {
                    // Visit parameter decorators
                    for decorator in &param.decorators {
                        visitor.visit_decorator(decorator);
                    }
                    visitor.visit_pattern(&param.pattern);
                    if let Some(type_ann) = &param.type_annotation {
                        visitor.visit_type_annotation(type_ann);
                    }
                }
                visitor.visit_block_statement(&ctor.body);
            }
        }
    }
}

pub fn walk_type_alias_decl<V: Visitor>(visitor: &mut V, decl: &TypeAliasDecl) {
    visitor.visit_identifier(&decl.name);
    visitor.visit_type_annotation(&decl.type_annotation);
}

pub fn walk_decorator<V: Visitor>(visitor: &mut V, decorator: &Decorator) {
    visitor.visit_expression(&decorator.expression);
}

pub fn walk_import_decl<V: Visitor>(_visitor: &mut V, _decl: &ImportDecl) {
    // Visit import specifiers if needed
}

pub fn walk_export_decl<V: Visitor>(visitor: &mut V, decl: &ExportDecl) {
    match decl {
        ExportDecl::Declaration(stmt) => visitor.visit_statement(stmt),
        ExportDecl::Named { .. } => {}
        ExportDecl::All { .. } => {}
    }
}

pub fn walk_if_statement<V: Visitor>(visitor: &mut V, stmt: &IfStatement) {
    visitor.visit_expression(&stmt.condition);
    visitor.visit_statement(&stmt.then_branch);
    if let Some(else_branch) = &stmt.else_branch {
        visitor.visit_statement(else_branch);
    }
}

pub fn walk_switch_statement<V: Visitor>(visitor: &mut V, stmt: &SwitchStatement) {
    visitor.visit_expression(&stmt.discriminant);
    for case in &stmt.cases {
        if let Some(test) = &case.test {
            visitor.visit_expression(test);
        }
        for consequent in &case.consequent {
            visitor.visit_statement(consequent);
        }
    }
}

pub fn walk_while_statement<V: Visitor>(visitor: &mut V, stmt: &WhileStatement) {
    visitor.visit_expression(&stmt.condition);
    visitor.visit_statement(&stmt.body);
}

pub fn walk_for_statement<V: Visitor>(visitor: &mut V, stmt: &ForStatement) {
    if let Some(init) = &stmt.init {
        match init {
            ForInit::VariableDecl(decl) => visitor.visit_variable_decl(decl),
            ForInit::Expression(expr) => visitor.visit_expression(expr),
        }
    }
    if let Some(test) = &stmt.test {
        visitor.visit_expression(test);
    }
    if let Some(update) = &stmt.update {
        visitor.visit_expression(update);
    }
    visitor.visit_statement(&stmt.body);
}

pub fn walk_block_statement<V: Visitor>(visitor: &mut V, stmt: &BlockStatement) {
    for statement in &stmt.statements {
        visitor.visit_statement(statement);
    }
}

pub fn walk_expression<V: Visitor>(visitor: &mut V, expr: &Expression) {
    match expr {
        Expression::IntLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::This(_) => {}
        Expression::Identifier(id) => visitor.visit_identifier(id),
        Expression::TemplateLiteral(lit) => {
            for part in &lit.parts {
                if let TemplatePart::Expression(expr) = part {
                    visitor.visit_expression(expr);
                }
            }
        }
        Expression::Array(arr) => visitor.visit_array_expression(arr),
        Expression::Object(obj) => visitor.visit_object_expression(obj),
        Expression::Unary(unary) => visitor.visit_expression(&unary.operand),
        Expression::Binary(binary) => visitor.visit_binary_expression(binary),
        Expression::Assignment(assign) => {
            visitor.visit_expression(&assign.left);
            visitor.visit_expression(&assign.right);
        }
        Expression::Logical(logical) => visitor.visit_logical_expression(logical),
        Expression::Conditional(cond) => {
            visitor.visit_expression(&cond.test);
            visitor.visit_expression(&cond.consequent);
            visitor.visit_expression(&cond.alternate);
        }
        Expression::Call(call) => visitor.visit_call_expression(call),
        Expression::AsyncCall(async_call) => {
            visitor.visit_expression(&async_call.callee);
            for arg in &async_call.arguments {
                visitor.visit_expression(arg);
            }
        }
        Expression::Member(member) => visitor.visit_member_expression(member),
        Expression::Index(index) => {
            visitor.visit_expression(&index.object);
            visitor.visit_expression(&index.index);
        }
        Expression::New(new_expr) => {
            visitor.visit_expression(&new_expr.callee);
            for arg in &new_expr.arguments {
                visitor.visit_expression(arg);
            }
        }
        Expression::Arrow(arrow) => visitor.visit_arrow_function(arrow),
        Expression::Await(await_expr) => visitor.visit_expression(&await_expr.argument),
        Expression::Typeof(typeof_expr) => visitor.visit_expression(&typeof_expr.argument),
        Expression::Parenthesized(paren) => visitor.visit_expression(&paren.expression),
        Expression::JsxElement(elem) => visitor.visit_jsx_element(elem),
        Expression::JsxFragment(frag) => visitor.visit_jsx_fragment(frag),
    }
}

pub fn walk_binary_expression<V: Visitor>(visitor: &mut V, expr: &BinaryExpression) {
    visitor.visit_expression(&expr.left);
    visitor.visit_expression(&expr.right);
}

pub fn walk_logical_expression<V: Visitor>(visitor: &mut V, expr: &LogicalExpression) {
    visitor.visit_expression(&expr.left);
    visitor.visit_expression(&expr.right);
}

pub fn walk_call_expression<V: Visitor>(visitor: &mut V, expr: &CallExpression) {
    visitor.visit_expression(&expr.callee);
    if let Some(type_args) = &expr.type_args {
        for arg in type_args {
            visitor.visit_type_annotation(arg);
        }
    }
    for arg in &expr.arguments {
        visitor.visit_expression(arg);
    }
}

pub fn walk_member_expression<V: Visitor>(visitor: &mut V, expr: &MemberExpression) {
    visitor.visit_expression(&expr.object);
    visitor.visit_identifier(&expr.property);
}

pub fn walk_array_expression<V: Visitor>(visitor: &mut V, expr: &ArrayExpression) {
    for elem in &expr.elements {
        if let Some(elem) = elem {
            match elem {
                ArrayElement::Expression(e) => visitor.visit_expression(e),
                ArrayElement::Spread(e) => visitor.visit_expression(e),
            }
        }
    }
}

pub fn walk_object_expression<V: Visitor>(visitor: &mut V, expr: &ObjectExpression) {
    for prop in &expr.properties {
        match prop {
            ObjectProperty::Property(p) => {
                // Visit computed property key if present
                if let PropertyKey::Computed(expr) = &p.key {
                    visitor.visit_expression(expr);
                }
                visitor.visit_expression(&p.value);
            }
            ObjectProperty::Spread(s) => {
                visitor.visit_expression(&s.argument);
            }
        }
    }
}

pub fn walk_arrow_function<V: Visitor>(visitor: &mut V, func: &ArrowFunction) {
    for param in &func.params {
        visitor.visit_pattern(&param.pattern);
        if let Some(type_ann) = &param.type_annotation {
            visitor.visit_type_annotation(type_ann);
        }
    }
    if let Some(return_type) = &func.return_type {
        visitor.visit_type_annotation(return_type);
    }
    match &func.body {
        ArrowBody::Expression(expr) => visitor.visit_expression(expr),
        ArrowBody::Block(block) => visitor.visit_block_statement(block),
    }
}

pub fn walk_jsx_element<V: Visitor>(visitor: &mut V, elem: &JsxElement) {
    for attr in &elem.opening.attributes {
        if let JsxAttribute::Spread { argument, .. } = attr {
            visitor.visit_expression(argument);
        }
    }
    for child in &elem.children {
        match child {
            JsxChild::Element(e) => visitor.visit_jsx_element(e),
            JsxChild::Fragment(f) => visitor.visit_jsx_fragment(f),
            JsxChild::Expression(e) => {
                if let Some(expr) = &e.expression {
                    visitor.visit_expression(expr);
                }
            }
            JsxChild::Text(_) => {}
        }
    }
}

pub fn walk_jsx_fragment<V: Visitor>(visitor: &mut V, frag: &JsxFragment) {
    for child in &frag.children {
        match child {
            JsxChild::Element(e) => visitor.visit_jsx_element(e),
            JsxChild::Fragment(f) => visitor.visit_jsx_fragment(f),
            JsxChild::Expression(e) => {
                if let Some(expr) = &e.expression {
                    visitor.visit_expression(expr);
                }
            }
            JsxChild::Text(_) => {}
        }
    }
}

pub fn walk_type_annotation<V: Visitor>(visitor: &mut V, ty: &TypeAnnotation) {
    match &ty.ty {
        Type::Primitive(_) => {}
        Type::Reference(type_ref) => {
            visitor.visit_identifier(&type_ref.name);
            if let Some(type_args) = &type_ref.type_args {
                for arg in type_args {
                    visitor.visit_type_annotation(arg);
                }
            }
        }
        Type::Union(union) => visitor.visit_union_type(union),
        Type::Function(func) => visitor.visit_function_type(func),
        Type::Array(arr) => visitor.visit_type_annotation(&arr.element_type),
        Type::Tuple(tuple) => {
            for elem in &tuple.element_types {
                visitor.visit_type_annotation(elem);
            }
        }
        Type::Object(obj) => visitor.visit_object_type(obj),
        Type::Typeof(typeof_ty) => visitor.visit_expression(&typeof_ty.argument),
        Type::StringLiteral(_) => {}
        Type::NumberLiteral(_) => {}
        Type::BooleanLiteral(_) => {}
        Type::Parenthesized(ty) => visitor.visit_type_annotation(ty),
    }
}

pub fn walk_union_type<V: Visitor>(visitor: &mut V, union: &UnionType) {
    for ty in &union.types {
        visitor.visit_type_annotation(ty);
    }
}

pub fn walk_function_type<V: Visitor>(visitor: &mut V, func: &FunctionType) {
    for param in &func.params {
        if let Some(name) = &param.name {
            visitor.visit_identifier(name);
        }
        visitor.visit_type_annotation(&param.ty);
    }
    visitor.visit_type_annotation(&func.return_type);
}

pub fn walk_object_type<V: Visitor>(visitor: &mut V, obj: &ObjectType) {
    for member in &obj.members {
        match member {
            ObjectTypeMember::Property(prop) => {
                visitor.visit_identifier(&prop.name);
                visitor.visit_type_annotation(&prop.ty);
            }
            ObjectTypeMember::Method(method) => {
                visitor.visit_identifier(&method.name);
                for param in &method.params {
                    if let Some(name) = &param.name {
                        visitor.visit_identifier(name);
                    }
                    visitor.visit_type_annotation(&param.ty);
                }
                visitor.visit_type_annotation(&method.return_type);
            }
        }
    }
}

pub fn walk_pattern<V: Visitor>(visitor: &mut V, pattern: &Pattern) {
    match pattern {
        Pattern::Identifier(id) => visitor.visit_identifier(id),
        Pattern::Array(arr) => {
            for elem in &arr.elements {
                if let Some(elem) = elem {
                    visitor.visit_pattern(&elem.pattern);
                    if let Some(default) = &elem.default {
                        visitor.visit_expression(default);
                    }
                }
            }
            if let Some(rest) = &arr.rest {
                visitor.visit_pattern(rest);
            }
        }
        Pattern::Object(obj) => {
            for prop in &obj.properties {
                visitor.visit_identifier(&prop.key);
                visitor.visit_pattern(&prop.value);
                if let Some(default) = &prop.default {
                    visitor.visit_expression(default);
                }
            }
            if let Some(rest) = &obj.rest {
                visitor.visit_identifier(rest);
            }
        }
    }
}
