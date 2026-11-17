use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Error, anyhow};
use swc_bundler::{Bundler, Hook, Load, ModuleData, ModuleRecord};
use swc_common::sync::Lrc;
use swc_common::{FileName, GLOBALS, Globals, Mark, SourceMap, Span};
use swc_ecma_ast::{EsVersion, KeyValueProp};
use swc_ecma_codegen::Emitter;
use swc_ecma_codegen::text_writer::JsWriter;
use swc_ecma_loader::TargetEnv;
use swc_ecma_loader::resolvers::node::NodeModulesResolver;
use swc_ecma_minifier::option::{
    CompressOptions, ExtraOptions, MangleOptions, MinifyOptions, TopLevelOptions,
};
use swc_ecma_parser::{Syntax, parse_file_as_module};
use swc_ecma_transforms_base::fixer::fixer;
use swc_ecma_visit::VisitMutWith;

struct NoMetaHook;

impl Hook for NoMetaHook {
    fn get_import_meta_props(
        &self,
        _span: Span,
        _module_record: &ModuleRecord,
    ) -> Result<Vec<KeyValueProp>, Error> {
        Ok(Vec::new())
    }
}

struct FileLoader {
    cm: Lrc<SourceMap>,
    es_version: EsVersion,
}

impl Load for FileLoader {
    fn load(&self, name: &FileName) -> Result<ModuleData, Error> {
        let FileName::Real(path) = name else {
            return Err(anyhow!("cannot load {name:?}"));
        };
        let fm = self
            .cm
            .load_file(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let module = parse_file_as_module(
            &fm,
            Syntax::Es(Default::default()),
            self.es_version,
            None,
            &mut Vec::new(),
        )
        .map_err(|e| anyhow!("parsing {}: {:?}", path.display(), e.kind()))?;
        Ok(ModuleData {
            fm,
            module,
            helpers: Default::default(),
        })
    }
}

pub struct Options {
    pub minify: bool,
    pub es_version: EsVersion,
    pub inline: bool,
    pub mangle_top_level: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            minify: false,
            es_version: EsVersion::latest(),
            inline: false,
            mangle_top_level: true,
        }
    }
}

pub fn bundle(entry: &Path, options: &Options) -> Result<String, Error> {
    let minify = options.minify;
    let cm: Lrc<SourceMap> = Default::default();
    let globals = Box::leak(Box::<Globals>::default());
    let mut bundler = Bundler::new(
        globals,
        cm.clone(),
        FileLoader {
            cm: cm.clone(),
            es_version: options.es_version,
        },
        NodeModulesResolver::new(TargetEnv::Node, Default::default(), true),
        swc_bundler::Config {
            require: false,
            disable_inliner: !options.inline,
            external_modules: Default::default(),
            disable_fixer: minify,
            disable_hygiene: minify,
            disable_dce: false,
            module: Default::default(),
        },
        Box::new(NoMetaHook),
    );
    let mut entries = HashMap::new();
    entries.insert("main".to_string(), FileName::Real(entry.to_path_buf()));
    let mut bundles = bundler.bundle(entries)?;
    let mut bundled = bundles
        .pop()
        .ok_or_else(|| anyhow!("bundler produced no output"))?;

    if minify {
        bundled.module = GLOBALS.set(globals, || {
            let mut module = swc_ecma_minifier::optimize(
                bundled.module.into(),
                cm.clone(),
                None,
                None,
                &MinifyOptions {
                    compress: Some(CompressOptions {
                        top_level: Some(TopLevelOptions {
                            functions: options.mangle_top_level,
                        }),
                        ..Default::default()
                    }),
                    mangle: Some(MangleOptions {
                        top_level: Some(options.mangle_top_level),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                &ExtraOptions {
                    unresolved_mark: Mark::new(),
                    top_level_mark: Mark::new(),
                    mangle_name_cache: None,
                },
            )
            .expect_module();
            module.visit_mut_with(&mut fixer(None));
            module
        });
    }

    let mut buf = Vec::new();
    {
        let mut emitter = Emitter {
            cfg: swc_ecma_codegen::Config::default().with_minify(minify),
            cm: cm.clone(),
            comments: None,
            wr: JsWriter::new(cm, "\n", &mut buf, None),
        };
        emitter.emit_module(&bundled.module)?;
    }
    Ok(String::from_utf8(buf)?)
}
