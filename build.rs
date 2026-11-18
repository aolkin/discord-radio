use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

fn content_hash(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn main() {
    let run_number = env::var("GITHUB_RUN_NUMBER").unwrap_or_else(|_| "dev".to_string());
    let commit_hash = env::var("GITHUB_SHA").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_RUN_NUMBER={run_number}");
    println!("cargo:rustc-env=BUILD_COMMIT_HASH={commit_hash}");
    println!("cargo:rerun-if-env-changed=GITHUB_RUN_NUMBER");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-changed=portal");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let static_dir = manifest_dir.join("portal/static");
    let bundle_dir = manifest_dir.join("portal/bundle");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("bundle");
    fs::create_dir_all(&out_dir).expect("creating the bundle output directory");

    let minify = env::var("PROFILE").is_ok_and(|profile| profile == "release");
    let entry = bundle_dir.join("index.js");
    let options = js_bundler::Options {
        minify,
        ..Default::default()
    };
    let js = js_bundler::bundle(&entry, &options)
        .unwrap_or_else(|e| panic!("bundling {}: {e:#}", entry.display()));
    fs::write(out_dir.join("index.js"), &js).expect("writing the bundled JavaScript");

    let mut assets = vec![("index.js".to_string(), content_hash(js.as_bytes()))];
    let mut static_names: Vec<String> = fs::read_dir(&static_dir)
        .unwrap_or_else(|e| panic!("listing {}: {e}", static_dir.display()))
        .map(|entry| entry.expect("reading a static directory entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "js" || ext == "css" || ext == "png")
        })
        .filter_map(|path| Some(path.file_name()?.to_str()?.to_string()))
        .collect();
    static_names.sort();
    for name in static_names {
        let hash = content_hash(&read(&static_dir.join(&name)));
        assets.push((name, hash));
    }

    let mut html = String::from_utf8(read(&bundle_dir.join("index.html")))
        .expect("portal/bundle/index.html is not UTF-8");
    for (name, hash) in assets {
        let quoted = format!("\"/static/{name}\"");
        assert!(
            html.contains(&quoted),
            "portal/bundle/index.html does not reference /static/{name}"
        );
        html = html.replace(&quoted, &format!("\"/static/{name}?v={hash}\""));
    }
    fs::write(out_dir.join("index.html"), html).expect("writing the generated index.html");
}
