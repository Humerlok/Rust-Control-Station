//! Embeds every vendored `.uavcan` definition so the catalog needs no files at run time.
use std::{
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

fn collect(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("read dsdl dir")
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, root, out);
        } else if path.extension().is_some_and(|e| e == "uavcan") {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, path));
        }
    }
}

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("dsdl");
    println!("cargo:rerun-if-changed=dsdl");
    let mut files = Vec::new();
    collect(&root, &root, &mut files);
    let mut code = String::from("pub const DSDL_FILES: &[(&str, &str)] = &[\n");
    for (relative, path) in &files {
        writeln!(
            code,
            "    ({relative:?}, include_str!({:?})),",
            path.to_string_lossy()
        )
        .unwrap();
    }
    code.push_str("];\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("dsdl_files.rs"),
        code,
    )
    .unwrap();
}
