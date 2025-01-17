use index_vec::IndexVec;
use oxc::{
  semantic::{ScopeId, SymbolId, SymbolTable},
  span::CompactString,
};

#[derive(Debug, Default)]
pub struct AstSymbols {
  pub names: IndexVec<SymbolId, CompactString>,
  pub scope_ids: IndexVec<SymbolId, ScopeId>,
}

impl AstSymbols {
  pub fn from_symbol_table(table: SymbolTable) -> Self {
    debug_assert!(table.references.is_empty());
    Self { names: table.names, scope_ids: table.scope_ids }
  }

  pub fn create_symbol(&mut self, name: CompactString, scope_id: ScopeId) -> SymbolId {
    self.scope_ids.push(scope_id);
    self.names.push(name)
  }

  pub fn scope_id_for(&self, symbol_id: SymbolId) -> ScopeId {
    self.scope_ids[symbol_id]
  }
}
