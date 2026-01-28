//! String interning for efficient token storage
//!
//! This module provides a string interner that deduplicates strings,
//! allowing us to use small integer symbols instead of owned strings.

use rustc_hash::FxHashMap;
use std::num::NonZeroU32;

/// An interned string symbol (32-bit index).
///
/// Symbols are small (4 bytes) and can be copied cheaply.
/// Use `Interner::resolve()` to get the actual string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Symbol(NonZeroU32);

impl Symbol {
    /// Create a symbol from a raw u32 (internal use only).
    #[inline]
    fn from_raw(raw: u32) -> Self {
        // Add 1 because NonZeroU32 cannot be 0
        Symbol(NonZeroU32::new(raw + 1).unwrap())
    }

    /// Get the raw index (internal use only).
    #[inline]
    fn to_raw(self) -> usize {
        (self.0.get() - 1) as usize
    }

    /// Create a dummy symbol (for error messages and tests)
    #[inline]
    pub const fn dummy() -> Self {
        // SAFETY: 1 is non-zero
        Symbol(unsafe { NonZeroU32::new_unchecked(1) })
    }
}

/// String interner that deduplicates strings.
///
/// Strings are stored once and referred to by small integer symbols.
/// This reduces memory usage and makes string comparison O(1).
#[derive(Clone)]
pub struct Interner {
    /// Map from string to symbol index
    map: FxHashMap<String, Symbol>,

    /// Vec of interned strings (indexed by symbol)
    strings: Vec<String>,
}

impl Interner {
    /// Create a new empty interner.
    pub fn new() -> Self {
        Self {
            map: FxHashMap::default(),
            strings: Vec::new(),
        }
    }

    /// Create a new interner with preallocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            map: FxHashMap::with_capacity_and_hasher(capacity, Default::default()),
            strings: Vec::with_capacity(capacity),
        }
    }

    /// Intern a string, returning its symbol.
    ///
    /// If the string was already interned, returns the existing symbol.
    /// Otherwise, allocates a new symbol and stores the string.
    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&sym) = self.map.get(s) {
            return sym;
        }

        let sym = Symbol::from_raw(self.strings.len() as u32);
        self.strings.push(s.to_string());
        self.map.insert(s.to_string(), sym);
        sym
    }

    /// Resolve a symbol back to its string.
    ///
    /// # Panics
    ///
    /// Panics if the symbol is invalid (not from this interner).
    #[inline]
    pub fn resolve(&self, sym: Symbol) -> &str {
        &self.strings[sym.to_raw()]
    }

    /// Get the number of interned strings.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Check if the interner is empty.
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_deduplicates() {
        let mut interner = Interner::new();

        let sym1 = interner.intern("hello");
        let sym2 = interner.intern("world");
        let sym3 = interner.intern("hello"); // Duplicate

        assert_eq!(sym1, sym3); // Same symbol
        assert_ne!(sym1, sym2); // Different symbols

        assert_eq!(interner.len(), 2); // Only 2 unique strings
    }

    #[test]
    fn test_resolve() {
        let mut interner = Interner::new();

        let sym = interner.intern("hello");
        assert_eq!(interner.resolve(sym), "hello");
    }

    #[test]
    fn test_multiple_strings() {
        let mut interner = Interner::new();

        let strings = vec!["foo", "bar", "baz", "foo", "bar"];
        let symbols: Vec<_> = strings.iter().map(|s| interner.intern(s)).collect();

        assert_eq!(symbols[0], symbols[3]); // Both "foo"
        assert_eq!(symbols[1], symbols[4]); // Both "bar"
        assert_eq!(interner.len(), 3); // Only 3 unique strings
    }

    #[test]
    fn test_symbol_is_copy() {
        let mut interner = Interner::new();
        let sym = interner.intern("test");

        // Symbols should be Copy
        let _sym2 = sym;
        let _sym3 = sym; // No move error

        assert_eq!(interner.resolve(sym), "test");
    }
}
