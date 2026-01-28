//! Builtin Type Signatures
//!
//! Defines structures for builtin type signatures that can be injected
//! into the binder/type checker.

/// Method signature for a builtin class
#[derive(Debug, Clone)]
pub struct BuiltinMethod {
    pub name: String,
    pub params: Vec<(String, String)>, // (name, type)
    pub return_type: String,
    pub is_static: bool,
}

/// Property signature for a builtin class
#[derive(Debug, Clone)]
pub struct BuiltinProperty {
    pub name: String,
    pub ty: String,
    pub is_static: bool,
}

/// Class signature for a builtin type
#[derive(Debug, Clone)]
pub struct BuiltinClass {
    pub name: String,
    pub type_params: Vec<String>,
    pub properties: Vec<BuiltinProperty>,
    pub methods: Vec<BuiltinMethod>,
    pub constructor_params: Option<Vec<(String, String)>>,
}

/// Function signature for a builtin function
#[derive(Debug, Clone)]
pub struct BuiltinFunction {
    pub name: String,
    pub type_params: Vec<String>,
    pub params: Vec<(String, String)>,
    pub return_type: String,
}

/// All signatures for a builtin module
#[derive(Debug, Clone)]
pub struct BuiltinSignatures {
    pub name: String,
    pub classes: Vec<BuiltinClass>,
    pub functions: Vec<BuiltinFunction>,
}

impl BuiltinSignatures {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            classes: Vec::new(),
            functions: Vec::new(),
        }
    }

    pub fn with_class(mut self, class: BuiltinClass) -> Self {
        self.classes.push(class);
        self
    }

    pub fn with_function(mut self, func: BuiltinFunction) -> Self {
        self.functions.push(func);
        self
    }
}

impl BuiltinClass {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_params: Vec::new(),
            properties: Vec::new(),
            methods: Vec::new(),
            constructor_params: None,
        }
    }

    pub fn with_type_params(mut self, params: Vec<&str>) -> Self {
        self.type_params = params.into_iter().map(String::from).collect();
        self
    }

    pub fn with_constructor(mut self, params: Vec<(&str, &str)>) -> Self {
        self.constructor_params = Some(
            params.into_iter().map(|(n, t)| (n.to_string(), t.to_string())).collect()
        );
        self
    }

    pub fn with_method(mut self, name: &str, params: Vec<(&str, &str)>, return_type: &str) -> Self {
        self.methods.push(BuiltinMethod {
            name: name.to_string(),
            params: params.into_iter().map(|(n, t)| (n.to_string(), t.to_string())).collect(),
            return_type: return_type.to_string(),
            is_static: false,
        });
        self
    }

    pub fn with_static_method(mut self, name: &str, params: Vec<(&str, &str)>, return_type: &str) -> Self {
        self.methods.push(BuiltinMethod {
            name: name.to_string(),
            params: params.into_iter().map(|(n, t)| (n.to_string(), t.to_string())).collect(),
            return_type: return_type.to_string(),
            is_static: true,
        });
        self
    }

    pub fn with_property(mut self, name: &str, ty: &str) -> Self {
        self.properties.push(BuiltinProperty {
            name: name.to_string(),
            ty: ty.to_string(),
            is_static: false,
        });
        self
    }
}

impl BuiltinFunction {
    pub fn new(name: &str, params: Vec<(&str, &str)>, return_type: &str) -> Self {
        Self {
            name: name.to_string(),
            type_params: Vec::new(),
            params: params.into_iter().map(|(n, t)| (n.to_string(), t.to_string())).collect(),
            return_type: return_type.to_string(),
        }
    }
}
