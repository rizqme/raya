//! Pretty-printing for IR
//!
//! Provides human-readable output for debugging IR structures.

use super::block::{BasicBlock, Terminator};
use super::function::IrFunction;
use super::instr::IrInstr;
use super::module::IrModule;
use super::value::{IrConstant, IrValue};
use std::fmt::Write;

/// Trait for pretty-printing IR constructs
pub trait PrettyPrint {
    fn pretty_print(&self) -> String;
}

impl PrettyPrint for IrModule {
    fn pretty_print(&self) -> String {
        let mut output = String::new();
        writeln!(output, "; module {}", self.name).unwrap();
        writeln!(output).unwrap();

        // Print classes
        for class in &self.classes {
            writeln!(output, "; class {}", class.name).unwrap();
            for field in &class.fields {
                writeln!(output, ";   field {}: type{}", field.name, field.ty.as_u32()).unwrap();
            }
            writeln!(output).unwrap();
        }

        // Print functions
        for func in &self.functions {
            output.push_str(&func.pretty_print());
            writeln!(output).unwrap();
        }

        output
    }
}

impl PrettyPrint for IrFunction {
    fn pretty_print(&self) -> String {
        let mut output = String::new();

        // Function signature
        let params: Vec<String> = self.params.iter().map(|p| format!("{}", p)).collect();
        writeln!(
            output,
            "fn {}({}) -> type{} {{",
            self.name,
            params.join(", "),
            self.return_ty.as_u32()
        )
        .unwrap();

        // Locals
        if !self.locals.is_empty() {
            write!(output, "  ; locals: ").unwrap();
            let locals: Vec<String> = self.locals.iter().map(|l| format!("{}", l)).collect();
            writeln!(output, "{}", locals.join(", ")).unwrap();
        }

        // Blocks
        for block in &self.blocks {
            output.push_str(&block.pretty_print_indented(2));
        }

        writeln!(output, "}}").unwrap();
        output
    }
}

impl BasicBlock {
    fn pretty_print_indented(&self, indent: usize) -> String {
        let mut output = String::new();
        let prefix = " ".repeat(indent);

        // Block header
        if let Some(label) = &self.label {
            writeln!(output, "{}{}: ; {}", prefix, self.id, label).unwrap();
        } else {
            writeln!(output, "{}{}:", prefix, self.id).unwrap();
        }

        // Instructions
        for instr in &self.instructions {
            writeln!(output, "{}  {}", prefix, format_instr(instr)).unwrap();
        }

        // Terminator
        writeln!(output, "{}  {}", prefix, self.terminator).unwrap();

        output
    }
}

fn format_instr(instr: &IrInstr) -> String {
    match instr {
        IrInstr::Assign { dest, value } => {
            format!("{} = {}", dest, format_value(value))
        }
        IrInstr::BinaryOp {
            dest,
            op,
            left,
            right,
        } => {
            format!("{} = {} {} {}", dest, left, op, right)
        }
        IrInstr::UnaryOp { dest, op, operand } => {
            format!("{} = {}{}", dest, op, operand)
        }
        IrInstr::Call { dest, func, args } => {
            let args_str: Vec<String> = args.iter().map(|a| format!("{}", a)).collect();
            if let Some(d) = dest {
                format!("{} = call {}({})", d, func, args_str.join(", "))
            } else {
                format!("call {}({})", func, args_str.join(", "))
            }
        }
        IrInstr::CallMethod {
            dest,
            object,
            method,
            args,
        } => {
            let args_str: Vec<String> = args.iter().map(|a| format!("{}", a)).collect();
            if let Some(d) = dest {
                format!(
                    "{} = call_method {}.method{}({})",
                    d,
                    object,
                    method,
                    args_str.join(", ")
                )
            } else {
                format!(
                    "call_method {}.method{}({})",
                    object,
                    method,
                    args_str.join(", ")
                )
            }
        }
        IrInstr::LoadLocal { dest, index } => {
            format!("{} = load_local {}", dest, index)
        }
        IrInstr::StoreLocal { index, value } => {
            format!("store_local {} = {}", index, value)
        }
        IrInstr::LoadGlobal { dest, index } => {
            format!("{} = load_global {}", dest, index)
        }
        IrInstr::StoreGlobal { index, value } => {
            format!("store_global {} = {}", index, value)
        }
        IrInstr::LoadField { dest, object, field } => {
            format!("{} = load_field {}.field{}", dest, object, field)
        }
        IrInstr::StoreField {
            object,
            field,
            value,
        } => {
            format!("store_field {}.field{} = {}", object, field, value)
        }
        IrInstr::LoadElement { dest, array, index } => {
            format!("{} = load_elem {}[{}]", dest, array, index)
        }
        IrInstr::StoreElement {
            array,
            index,
            value,
        } => {
            format!("store_elem {}[{}] = {}", array, index, value)
        }
        IrInstr::NewObject { dest, class } => {
            format!("{} = new_object {}", dest, class)
        }
        IrInstr::NewArray { dest, len, elem_ty } => {
            format!("{} = new_array type{}[{}]", dest, elem_ty.as_u32(), len)
        }
        IrInstr::ArrayLiteral {
            dest,
            elements,
            elem_ty,
        } => {
            let elems: Vec<String> = elements.iter().map(|e| format!("{}", e)).collect();
            format!(
                "{} = array_literal type{}[{}]",
                dest,
                elem_ty.as_u32(),
                elems.join(", ")
            )
        }
        IrInstr::ObjectLiteral { dest, class, fields } => {
            let field_strs: Vec<String> = fields
                .iter()
                .map(|(idx, val)| format!("field{}: {}", idx, val))
                .collect();
            format!(
                "{} = object_literal {} {{ {} }}",
                dest,
                class,
                field_strs.join(", ")
            )
        }
        IrInstr::ArrayLen { dest, array } => {
            format!("{} = array_len {}", dest, array)
        }
        IrInstr::StringLen { dest, string } => {
            format!("{} = string_len {}", dest, string)
        }
        IrInstr::Typeof { dest, operand } => {
            format!("{} = typeof {}", dest, operand)
        }
        IrInstr::Phi { dest, sources } => {
            let srcs: Vec<String> = sources
                .iter()
                .map(|(block, reg)| format!("[{}: {}]", block, reg))
                .collect();
            format!("{} = phi {}", dest, srcs.join(", "))
        }
        IrInstr::MakeClosure { dest, func, captures } => {
            let caps: Vec<String> = captures.iter().map(|c| format!("{}", c)).collect();
            format!("{} = make_closure {}({})", dest, func, caps.join(", "))
        }
        IrInstr::LoadCaptured { dest, index } => {
            format!("{} = load_captured {}", dest, index)
        }
        IrInstr::StoreCaptured { index, value } => {
            format!("store_captured {} = {}", index, value)
        }
        IrInstr::SetClosureCapture { closure, index, value } => {
            format!("set_closure_capture {}.captures[{}] = {}", closure, index, value)
        }
        IrInstr::NewRefCell { dest, initial_value } => {
            format!("{} = new_refcell({})", dest, initial_value)
        }
        IrInstr::LoadRefCell { dest, refcell } => {
            format!("{} = load_refcell({})", dest, refcell)
        }
        IrInstr::StoreRefCell { refcell, value } => {
            format!("store_refcell {} = {}", refcell, value)
        }
        IrInstr::CallClosure { dest, closure, args } => {
            let args_str: Vec<String> = args.iter().map(|a| format!("{}", a)).collect();
            if let Some(d) = dest {
                format!("{} = call_closure {}({})", d, closure, args_str.join(", "))
            } else {
                format!("call_closure {}({})", closure, args_str.join(", "))
            }
        }
        IrInstr::StringCompare { dest, left, right, mode, negate } => {
            let op = if *negate { "!=" } else { "==" };
            format!("{} = string_compare({}) {} {} {}", dest, mode, left, op, right)
        }
        IrInstr::ToString { dest, operand } => {
            format!("{} = to_string {}", dest, operand)
        }
    }
}

fn format_value(value: &IrValue) -> String {
    match value {
        IrValue::Register(reg) => format!("{}", reg),
        IrValue::Constant(c) => format!("{}", c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::block::BasicBlockId;
    use crate::ir::instr::BinaryOp;
    use crate::ir::value::{Register, RegisterId};
    use raya_parser::TypeId;

    fn make_reg(id: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(0))
    }

    #[test]
    fn test_pretty_print_assign() {
        let instr = IrInstr::Assign {
            dest: make_reg(0),
            value: IrValue::Constant(IrConstant::I32(42)),
        };
        assert_eq!(format_instr(&instr), "r0:0 = 42");
    }

    #[test]
    fn test_pretty_print_binary_op() {
        let instr = IrInstr::BinaryOp {
            dest: make_reg(2),
            op: BinaryOp::Add,
            left: make_reg(0),
            right: make_reg(1),
        };
        assert_eq!(format_instr(&instr), "r2:0 = r0:0 + r1:0");
    }

    #[test]
    fn test_pretty_print_function() {
        let mut func = IrFunction::new("add", vec![make_reg(0), make_reg(1)], TypeId::new(1));
        let mut block = BasicBlock::new(BasicBlockId(0));
        block.set_terminator(Terminator::Return(Some(make_reg(2))));
        func.add_block(block);

        let output = func.pretty_print();
        assert!(output.contains("fn add"));
        assert!(output.contains("return"));
    }
}
