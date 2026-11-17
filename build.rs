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
    println!("cargo:rerun-if-changed=static");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let static_dir = manifest_dir.join("static");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("portal");
    fs::create_dir_all(&out_dir).expect("creating the portal output directory");

    let minify = env::var("PROFILE").is_ok_and(|profile| profile == "release");
    let entry = static_dir.join("index.js");
    let options = js_bundler::Options {
        minify,
        ..Default::default()
    };
    let js = js_bundler::bundle(&entry, &options)
        .unwrap_or_else(|e| panic!("bundling {}: {e:#}", entry.display()));
    fs::write(out_dir.join("index.js"), &js).expect("writing the bundled JavaScript");

    let mut assets = vec![("index.js".to_string(), content_hash(js.as_bytes()))];
    for name in ["theme.js", "alpine.min.js", "base.css", "index.css"] {
        let bytes = read(&static_dir.join(name));
        fs::write(out_dir.join(name), &bytes)
            .unwrap_or_else(|e| panic!("writing {name} to the portal output directory: {e}"));
        assets.push((name.to_string(), content_hash(&bytes)));
    }

    let mut html = String::from_utf8(read(&static_dir.join("index.html")))
        .expect("static/index.html is not UTF-8");
    for (name, hash) in assets {
        let quoted = format!("\"/static/{name}\"");
        assert!(
            html.contains(&quoted),
            "static/index.html does not reference /static/{name}"
        );
        html = html.replace(&quoted, &format!("\"/static/{name}?v={hash}\""));
    }
    fs::write(out_dir.join("index.html"), html).expect("writing the generated index.html");
}
