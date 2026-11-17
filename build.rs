use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Error, anyhow};
use swc_bundler::{Bundler, Hook, Load, ModuleData, ModuleRecord};
use swc_common::sync::Lrc;
use swc_common::{FileName, GLOBALS, Globals, Mark, SourceMap, Span};
use swc_ecma_ast::{EsVersion, KeyValueProp};
use swc_ecma_codegen::Emitter;
use swc_ecma_codegen::text_writer::JsWriter;
use swc_ecma_loader::resolve::{Resolution, Resolve};
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

struct RelativeResolver;

impl Resolve for RelativeResolver {
    fn resolve(&self, base: &FileName, module_specifier: &str) -> Result<Resolution, Error> {
        let FileName::Real(base_path) = base else {
            return Err(anyhow!("cannot resolve imports from {base:?}"));
        };
        if !module_specifier.starts_with("./") && !module_specifier.starts_with("../") {
            return Err(anyhow!("unsupported import specifier {module_specifier}"));
        }
        let dir = base_path
            .parent()
            .ok_or_else(|| anyhow!("{} has no parent directory", base_path.display()))?;
        let path = dir.join(module_specifier).canonicalize().with_context(|| {
            format!("resolving {module_specifier} from {}", base_path.display())
        })?;
        Ok(Resolution {
            filename: FileName::Real(path),
            slug: None,
        })
    }
}

struct FileLoader {
    cm: Lrc<SourceMap>,
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
            EsVersion::Es2020,
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

fn bundle(entry: &Path, minify: bool) -> Result<String, Error> {
    let cm: Lrc<SourceMap> = Default::default();
    let globals = Box::leak(Box::<Globals>::default());
    let mut bundler = Bundler::new(
        globals,
        cm.clone(),
        FileLoader { cm: cm.clone() },
        RelativeResolver,
        swc_bundler::Config {
            require: false,
            disable_inliner: true,
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
                        top_level: Some(TopLevelOptions { functions: true }),
                        ..Default::default()
                    }),
                    mangle: Some(MangleOptions {
                        top_level: Some(true),
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

fn content_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn file_hash(path: &Path) -> String {
    content_hash(&fs::read(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display())))
}

fn main() {
    let run_number = env::var("GITHUB_RUN_NUMBER").unwrap_or_else(|_| "dev".to_string());
    let commit_hash = env::var("GITHUB_SHA").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_RUN_NUMBER={run_number}");
    println!("cargo:rustc-env=BUILD_COMMIT_HASH={commit_hash}");
    println!("cargo:rerun-if-env-changed=GITHUB_RUN_NUMBER");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-changed=static");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let static_dir = manifest_dir.join("static");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("portal");
    fs::create_dir_all(&out_dir).expect("creating the portal output directory");

    let minify = env::var("PROFILE").is_ok_and(|profile| profile == "release");
    let entry = static_dir.join("index.js");
    let js =
        bundle(&entry, minify).unwrap_or_else(|e| panic!("bundling {}: {e:#}", entry.display()));
    fs::write(out_dir.join("index.js"), &js).expect("writing the bundled JavaScript");

    let mut html =
        fs::read_to_string(static_dir.join("index.html")).expect("reading static/index.html");
    let assets = [
        ("/static/index.js", content_hash(js.as_bytes())),
        ("/static/theme.js", file_hash(&static_dir.join("theme.js"))),
        (
            "/static/alpine.min.js",
            file_hash(&static_dir.join("alpine.min.js")),
        ),
        ("/static/base.css", file_hash(&static_dir.join("base.css"))),
        (
            "/static/index.css",
            file_hash(&static_dir.join("index.css")),
        ),
    ];
    for (url, hash) in assets {
        let quoted = format!("\"{url}\"");
        assert!(
            html.contains(&quoted),
            "static/index.html does not reference {url}"
        );
        html = html.replace(&quoted, &format!("\"{url}?v={hash}\""));
    }
    fs::write(out_dir.join("index.html"), html).expect("writing the generated index.html");
}
