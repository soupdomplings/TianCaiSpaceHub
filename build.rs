fn main() {
    embed_remote_web();
    println!("cargo:rerun-if-changed=assets/tci-hub-icon.png");
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        println!(
            "cargo:rustc-link-arg-bin=TianCaiSpaceHub=/MANIFESTINPUT:packaging/windows/TianCaiSpaceHub.exe.manifest"
        );
        println!("cargo:rustc-link-arg-bin=TianCaiSpaceHub=/MANIFEST:EMBED");
        println!("cargo:rerun-if-changed=packaging/windows/TianCaiSpaceHub.exe.manifest");
        println!("cargo:rerun-if-changed=packaging/windows/TianCaiSpaceHub.rc");
        println!("cargo:rerun-if-changed=packaging/icons/AppIcon.ico");
        let version = std::env::var("CARGO_PKG_VERSION").unwrap();
        let version_numbers = ["MAJOR", "MINOR", "PATCH"]
            .map(|part| std::env::var(format!("CARGO_PKG_VERSION_{part}")).unwrap())
            .join(",");
        // Windows Installer compares the fixed numeric file version, not the
        // display string. Preserve the custom revision so -4 and -5 differ.
        let revision = std::env::var("CARGO_PKG_VERSION_PRE").unwrap();
        let revision = if revision.is_empty() {
            0
        } else {
            revision
                .parse::<u16>()
                .expect("Windows custom revision must be an integer from 0 to 65535")
        };
        let definitions = [
            format!("HUB_VERSION_STRING=\"{version}\""),
            format!("HUB_VERSION_NUMBERS={version_numbers},{revision}"),
        ];
        embed_resource::compile("packaging/windows/TianCaiSpaceHub.rc", &definitions)
            .manifest_optional()
            .unwrap();
    }
}

fn embed_remote_web() {
    use std::{fmt::Write, path::Path};
    let root = Path::new("assets/remote-web");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut entries = Vec::new();
    fn collect(root: &Path, directory: &Path, entries: &mut Vec<(String, String)>) {
        if !directory.exists() {
            return;
        }
        for entry in std::fs::read_dir(directory).expect("read remote web resources") {
            let path = entry.expect("remote web resource entry").path();
            if path.is_dir() {
                collect(root, &path, entries);
            } else if path.is_file() {
                entries.push((
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    path.canonicalize().unwrap().to_string_lossy().to_string(),
                ));
            }
        }
    }
    collect(root, root, &mut entries);
    assert!(
        entries.iter().any(|(name, _)| name == "index.html"),
        "remote web resources are missing; run npm ci and npm run build in web-remote"
    );
    entries.sort();
    let mut source = String::from("pub(super) static ASSETS: &[(&str, &[u8])] = &[\n");
    for (name, path) in entries {
        writeln!(&mut source, "({name:?}, include_bytes!({path:?})),").unwrap();
    }
    source.push_str("];");
    std::fs::write(
        Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("remote_web_assets.rs"),
        source,
    )
    .expect("embed remote web resources");
}
