use oxc::span::Atom;
use rolldown_common::{ModuleId, NamedImport, Specifier, SymbolRef};
use rolldown_error::BuildError;
use rolldown_sourcemap::{collapse_sourcemaps, concat_sourcemaps, SourceMap};
use rustc_hash::FxHashMap;

use crate::{
  bundler::{
    bundle::output::RenderedModule, chunk_graph::ChunkGraph, module::ModuleRenderContext,
    options::output_options::OutputOptions, stages::link_stage::LinkStageOutput,
    utils::bitset::BitSet,
  },
  error::BatchedResult,
  FileNameTemplate, InputOptions,
};

use super::ChunkId;

#[derive(Debug)]
pub struct CrossChunkImportItem {
  pub export_alias: Option<Specifier>,
  pub import_ref: SymbolRef,
}

#[derive(Debug)]
pub enum ChunkKind {
  EntryPoint { is_user_defined: bool, bit: u32, module: ModuleId },
  Common,
}

impl Default for ChunkKind {
  fn default() -> Self {
    Self::Common
  }
}

#[derive(Debug, Default)]
pub struct Chunk {
  pub kind: ChunkKind,
  pub modules: Vec<ModuleId>,
  pub name: Option<String>,
  pub file_name: Option<String>,
  pub canonical_names: FxHashMap<SymbolRef, Atom>,
  pub bits: BitSet,
  pub imports_from_other_chunks: FxHashMap<ChunkId, Vec<CrossChunkImportItem>>,
  pub imports_from_external_modules: FxHashMap<ModuleId, Vec<NamedImport>>,
  // meaningless if the chunk is an entrypoint
  pub exports_to_other_chunks: FxHashMap<SymbolRef, Atom>,
}

impl Chunk {
  pub fn new(name: Option<String>, bits: BitSet, modules: Vec<ModuleId>, kind: ChunkKind) -> Self {
    Self { modules, name, bits, kind, ..Self::default() }
  }

  pub fn file_name_template<'a>(
    &mut self,
    output_options: &'a OutputOptions,
  ) -> &'a FileNameTemplate {
    if matches!(self.kind, ChunkKind::EntryPoint { is_user_defined, .. } if is_user_defined) {
      &output_options.entry_file_names
    } else {
      &output_options.chunk_file_names
    }
  }

  #[allow(clippy::unnecessary_wraps, clippy::cast_possible_truncation, clippy::type_complexity)]
  pub fn render(
    &self,
    input_options: &InputOptions,
    graph: &LinkStageOutput,
    chunk_graph: &ChunkGraph,
    output_options: &OutputOptions,
  ) -> BatchedResult<((String, Option<SourceMap>), FxHashMap<String, RenderedModule>)> {
    use rayon::prelude::*;
    let mut rendered_modules = FxHashMap::default();
    let mut content_and_sourcemaps = vec![];

    content_and_sourcemaps
      .push((self.render_imports_for_esm(graph, chunk_graph).to_string(), None));

    self
      .modules
      .par_iter()
      .copied()
      .map(|id| &graph.modules[id])
      .filter_map(|m| match m {
        crate::bundler::module::Module::Normal(m) => {
          let rendered_content = m.render(
            &ModuleRenderContext {
              canonical_names: &self.canonical_names,
              graph,
              chunk_graph,
              input_options,
            },
            &graph.ast_table[m.id],
          );
          Some((
            m.resource_id.expect_file().to_string(),
            RenderedModule {
              original_length: m.source.len().try_into().unwrap(),
              rendered_length: rendered_content
                .as_ref()
                .map(|c| c.len() as u32)
                .unwrap_or_default(),
            },
            rendered_content,
            if output_options.sourcemap.is_hidden() {
              None
            } else {
              // TODO add oxc codegen sourcemap to sourcemap chain
              Some(collapse_sourcemaps(m.sourcemap_chain.clone()))
            },
          ))
        }
        crate::bundler::module::Module::External(_) => None,
      })
      .collect::<Vec<_>>()
      .into_iter()
      .try_for_each(
        |(module_path, rendered_module, rendered_content, map)| -> Result<(), BuildError> {
          if let Some(rendered_content) = rendered_content {
            content_and_sourcemaps.push((
              rendered_content.to_string(),
              match map {
                None => None,
                Some(v) => v?,
              },
            ));
          }
          rendered_modules.insert(module_path, rendered_module);
          Ok(())
        },
      )?;

    if let Some(exports) = self.render_exports(graph, output_options) {
      content_and_sourcemaps.push((exports.to_string(), None));
    }

    if output_options.sourcemap.is_hidden() {
      return Ok((
        (content_and_sourcemaps.into_iter().map(|(c, _)| c).collect::<Vec<_>>().join("\n"), None),
        rendered_modules,
      ));
    }

    let (content, map) = concat_sourcemaps(&content_and_sourcemaps)?;
    Ok(((content, Some(map)), rendered_modules))
  }

  pub fn is_entry_point(&self) -> bool {
    matches!(self.kind, ChunkKind::EntryPoint { .. })
  }
}
