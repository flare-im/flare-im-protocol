//! Reading and writing web bundle archives (`--features bundle-archive`):
//! the platform's static check and the client cache's unpacking.
#![cfg(feature = "bundle-archive")]
use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use flare_plugin_protocol::{PluginManifest, bundle::*};
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const INDEX: &[u8] = br#"<!doctype html><html><head><meta charset="utf-8">
<script type="module" src="./assets/index.js"></script>
<link rel="stylesheet" href="./assets/index.css"></head><body><div id="app"></div></body></html>"#;

fn plugin_manifest() -> PluginManifest {
    let mut manifest: Value =
        serde_json::from_slice(include_bytes!("fixtures/plugin-ui-v2-manifest.json")).unwrap();
    manifest["capabilities"][0]["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": "dash", "idempotent": true, "inputSchema": {"type": "object"},
            "outputSchema": {"type": "object"}, "permissionIds": ["example.poll.read"],
            "timeoutMs": 5000, "mode": "query"
        }));
    manifest["clientExtensions"]["contributions"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "kind": "page", "id": "example.poll.dashboard", "schemaVersion": 1,
            "requiredPermissions": ["example.poll.read"], "schema": {"title": "Dashboard"},
            "variants": [
                {"id": "bundle", "requires": {"bundle": {"id": "dashboard", "bridge": 1}},
                 "bundle": {"id": "dashboard", "route": "/"}},
                {"id": "basic"}
            ]
        }));
    manifest["clientExtensions"]["webBundles"] = json!([{
        "id": "dashboard", "sha256": "0".repeat(64), "size": 1, "bridge": 1
    }]);
    PluginManifest::from_json(&serde_json::to_vec(&manifest).unwrap()).unwrap()
}

fn bundle_manifest() -> BundleManifest {
    serde_json::from_value(json!({
        "id": "dashboard", "bridge": 1,
        "permissions": ["plugin.query"], "operations": ["example.poll.dash"],
        "locales": ["en"]
    }))
    .unwrap()
}

fn files(extra: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
    let mut files: BTreeMap<String, Vec<u8>> = [
        ("index.html", INDEX),
        ("assets/index.js", b"console.log(1)".as_slice()),
        ("assets/index.css", b"body{margin:0}".as_slice()),
    ]
    .into_iter()
    .chain(extra.iter().copied())
    .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
    .collect();
    files.retain(|_, bytes| !bytes.is_empty());
    files
}

fn decl_of(bytes: &[u8]) -> WebBundleDecl {
    serde_json::from_value(json!({
        "id": "dashboard", "sha256": sha256_hex(bytes), "size": bytes.len(), "bridge": 1
    }))
    .unwrap()
}

fn check(bytes: &[u8]) -> Result<BundleIndex, BundleError> {
    check_archive(bytes, &decl_of(bytes), &plugin_manifest())
}

/// A stored zip of `entries` as given, so tests can build what
/// [`write_archive`] refuses to.
fn raw_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// Every occurrence of `from` replaced by `to` (same length): a name in the
/// local and the central headers alike.
fn replace(bytes: &mut [u8], from: &[u8], to: &[u8]) {
    assert_eq!(from.len(), to.len());
    let mut found = 0;
    for start in 0..=bytes.len() - from.len() {
        if &bytes[start..start + from.len()] == from {
            bytes[start..start + to.len()].copy_from_slice(to);
            found += 1;
        }
    }
    assert!(found >= 2, "local and central header");
}

fn bundle_json() -> Vec<u8> {
    serde_json::to_vec(&bundle_manifest()).unwrap()
}

#[test]
fn a_written_bundle_passes_the_check_and_lists_its_files() {
    let bytes = write_archive(&bundle_manifest(), &files(&[])).unwrap();
    assert_eq!(
        bytes,
        write_archive(&bundle_manifest(), &files(&[])).unwrap(),
        "the same contents give the same bytes"
    );
    let index = check(&bytes).unwrap();
    assert_eq!(index.manifest, bundle_manifest());
    assert_eq!(index.sha256, sha256_hex(&bytes));
    assert_eq!(index.size, bytes.len() as u64);
    let paths: Vec<_> = index.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "assets/index.css",
            "assets/index.js",
            "bundle.json",
            "index.html"
        ]
    );
    let html = index.file("index.html").unwrap();
    assert_eq!(html.sha256, sha256_hex(INDEX));
    assert_eq!(html.size, INDEX.len() as u64);
    assert_eq!(html.content_type, "text/html; charset=utf-8");
    assert_eq!(
        index.unpacked_size,
        index.files.iter().map(|f| f.size).sum::<u64>()
    );
    assert!(index.warnings.is_empty());

    // The client cache unpacks the same bundle without the plugin manifest.
    let unpacked = unpack_archive(&bytes, &sha256_hex(&bytes), bytes.len() as u64).unwrap();
    assert_eq!(unpacked.index, index.files);
    assert_eq!(unpacked.files["assets/index.js"], b"console.log(1)");
}

#[test]
fn the_declaration_digest_and_size_are_checked_first() {
    let bytes = write_archive(&bundle_manifest(), &files(&[])).unwrap();
    let manifest = plugin_manifest();
    let mut wrong = decl_of(&bytes);
    wrong.sha256 = "f".repeat(64);
    assert_eq!(
        check_archive(&bytes, &wrong, &manifest).unwrap_err(),
        BundleError::DigestMismatch
    );
    let mut wrong = decl_of(&bytes);
    wrong.size += 1;
    assert_eq!(
        check_archive(&bytes, &wrong, &manifest).unwrap_err(),
        BundleError::SizeMismatch
    );
    let mut flipped = bytes.clone();
    let last = flipped.len() - 30;
    flipped[last] ^= 1;
    assert_eq!(
        unpack_archive(&flipped, &sha256_hex(&bytes), bytes.len() as u64).unwrap_err(),
        BundleError::DigestMismatch,
        "one changed byte breaks the digest chain"
    );
    let too_large = vec![0u8; MAX_WEB_BUNDLE_BYTES as usize + 1];
    assert_eq!(
        unpack_archive(&too_large, &sha256_hex(&too_large), too_large.len() as u64).unwrap_err(),
        BundleError::TooLarge
    );
    assert!(matches!(
        unpack_archive(b"not a zip", &sha256_hex(b"not a zip"), 9),
        Err(BundleError::InvalidArchive(_))
    ));
}

#[test]
fn the_archive_structure_paths_and_types_are_checked() {
    let json = bundle_json();
    let base: Vec<(&str, &[u8])> = vec![
        ("bundle.json", json.as_slice()),
        ("index.html", INDEX),
        ("assets/index.js", b"x".as_slice()),
    ];
    let with = |extra: (&str, &[u8])| {
        let mut entries = base.clone();
        entries.push(extra);
        check(&raw_zip(&entries))
    };
    assert!(check(&raw_zip(&base)).is_ok());
    // Paths that leave the bundle, written as a plain name first.
    for (written, hostile) in [
        ("assets/xx/escape.js", "assets/../escape.js"),
        ("xetc/passwd.js", "/etc/passwd.js"),
        ("assets/Xndex.js", "assets/Index.js"),
    ] {
        let mut entries = base.clone();
        entries.push((written, b"x".as_slice()));
        let mut bytes = raw_zip(&entries);
        replace(&mut bytes, written.as_bytes(), hostile.as_bytes());
        assert_eq!(
            check(&bytes).unwrap_err(),
            BundleError::InvalidPath(hostile.into()),
            "{hostile}"
        );
    }
    // Links and directories are not plain files.
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in &base {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer
        .add_symlink("assets/link.js", "/etc/passwd", options)
        .unwrap();
    assert!(matches!(
        check(&writer.finish().unwrap().into_inner()),
        Err(BundleError::InvalidArchive(_))
    ));
    assert_eq!(
        with(("assets/engine.wasm", b"\0asm".as_slice())).unwrap_err(),
        BundleError::FileType("assets/engine.wasm".into())
    );
    assert_eq!(
        with(("assets/other.html", b"<p>".as_slice())).unwrap_err(),
        BundleError::FileType("assets/other.html".into())
    );
    let big = vec![b'a'; MAX_WEB_BUNDLE_FILE_BYTES as usize + 1];
    assert_eq!(
        with(("assets/big.js", big.as_slice())).unwrap_err(),
        BundleError::FileTooLarge("assets/big.js".into())
    );
    // Missing bundle.json or entry.
    assert_eq!(
        check(&raw_zip(&base[1..])).unwrap_err(),
        BundleError::MissingFile("bundle.json".into())
    );
    assert_eq!(
        check(&raw_zip(&[base[0], base[2]])).unwrap_err(),
        BundleError::MissingFile("index.html".into())
    );
    // Too many files.
    let names: Vec<String> = (0..MAX_WEB_BUNDLE_FILES)
        .map(|i| format!("assets/{i}.js"))
        .collect();
    let mut many = base.clone();
    many.extend(names.iter().map(|name| (name.as_str(), b"x".as_slice())));
    assert_eq!(
        check(&raw_zip(&many)).unwrap_err(),
        BundleError::TooManyFiles
    );
}

#[test]
fn the_entry_document_and_bundle_json_are_checked_against_the_manifest() {
    let inline = files(&[(
        "index.html",
        b"<script>fetch('https://x')</script>".as_slice(),
    )]);
    assert_eq!(
        check(&write_archive(&bundle_manifest(), &inline).unwrap()).unwrap_err(),
        BundleError::Html(HtmlViolation::InlineScript)
    );
    let external = files(&[(
        "index.html",
        br#"<script src="https://cdn.example.com/vue.js"></script>"#.as_slice(),
    )]);
    assert!(matches!(
        check(&write_archive(&bundle_manifest(), &external).unwrap()).unwrap_err(),
        BundleError::Html(HtmlViolation::ExternalScript(_))
    ));
    let mut admin = bundle_manifest();
    admin.operations = ["example.poll.list".to_string()].into_iter().collect();
    assert_eq!(
        check(&write_archive(&admin, &files(&[])).unwrap()).unwrap_err(),
        BundleError::UndeclaredOperation("example.poll.list".into())
    );
    let mut other = bundle_manifest();
    other.id = "other".into();
    assert_eq!(
        check(&write_archive(&other, &files(&[])).unwrap()).unwrap_err(),
        BundleError::DeclarationMismatch("id")
    );
    // Dynamic code is reported, not refused.
    let eval = files(&[("assets/index.js", b"const x = eval(input)".as_slice())]);
    let index = check(&write_archive(&bundle_manifest(), &eval).unwrap()).unwrap();
    assert_eq!(
        index.warnings,
        [BundleWarning {
            path: "assets/index.js".into(),
            code: BundleWarningCode::Eval
        }]
    );
}

#[test]
fn write_archive_refuses_what_the_check_would() {
    assert!(matches!(
        write_archive(
            &bundle_manifest(),
            &files(&[("assets/a.wasm", b"\0asm".as_slice())])
        ),
        Err(BundleError::FileType(_))
    ));
    assert!(matches!(
        write_archive(
            &bundle_manifest(),
            &files(&[("bundle.json", b"{}".as_slice())])
        ),
        Err(BundleError::InvalidArchive(_))
    ));
    let mut no_entry = files(&[]);
    no_entry.remove("index.html");
    assert_eq!(
        write_archive(&bundle_manifest(), &no_entry).unwrap_err(),
        BundleError::MissingFile("index.html".into())
    );
}
