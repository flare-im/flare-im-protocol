//! Web bundles (L3): plugin pages written in HTML, CSS and JavaScript that a
//! host runs in a sandboxed web view and that reach the plugin only through
//! the host bridge ([`crate::bridge`]).
//!
//! ```text
//! dashboard.fwb       a zip of stored (uncompressed) entries
//!   bundle.json       BundleManifest: entry, bridge, permissions, operations
//!   index.html        the entry, the only HTML file
//!   assets/*.js|*.css|*.woff2|*.png|*.svg|*.webp|*.json
//! ```
//!
//! The plugin manifest declares each bundle ([`WebBundleDecl`]) with the
//! SHA-256 of the file; the publisher signature over the manifest covers the
//! digest and so the bundle. The file itself travels through the artifact
//! pipeline. The digest is lowercase hex without a prefix, the same value in
//! the manifest, the platform's read-back, the catalog and the client cache.
//!
//! Always compiled: the types, the limits, the path and type allowlists, the
//! `bundle.json` rules ([`BundleManifest::validate_for`]), the entry HTML
//! rules ([`check_entry_html`]) and the script scan ([`scan_script`]). Behind
//! the `bundle-archive` feature: reading and writing the zip
//! ([`check_archive`], [`unpack_archive`], [`write_archive`]), so a client
//! that never opens an archive (the browser engine reads unpacked files)
//! does not compile a zip reader.
use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::PluginManifest;
use crate::bridge::{BRIDGE_VERSION, BridgePermission};
use crate::ui::NativeCapability;

/// File extension of a web bundle.
pub const WEB_BUNDLE_EXTENSION: &str = "fwb";
/// The bundle manifest entry.
pub const BUNDLE_MANIFEST_ENTRY: &str = "bundle.json";
/// The default entry document.
pub const DEFAULT_BUNDLE_ENTRY: &str = "index.html";
/// A bundle file (D-05).
pub const MAX_WEB_BUNDLE_BYTES: u64 = 10 * 1024 * 1024;
/// The sum of the unpacked entries (D-05).
pub const MAX_WEB_BUNDLE_UNPACKED_BYTES: u64 = 20 * 1024 * 1024;
/// Entries of a bundle, `bundle.json` included.
pub const MAX_WEB_BUNDLE_FILES: usize = 512;
/// One entry.
pub const MAX_WEB_BUNDLE_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Bytes of an entry path.
pub const MAX_WEB_BUNDLE_PATH_BYTES: usize = 256;
/// Bytes of `bundle.json`.
pub const MAX_BUNDLE_MANIFEST_BYTES: usize = 64 * 1024;
/// Web bundles one plugin version declares.
pub const MAX_WEB_BUNDLES: usize = 8;
/// Operations one `bundle.json` lists.
pub const MAX_BUNDLE_OPERATIONS: usize = 64;
/// Locales one `bundle.json` lists.
pub const MAX_BUNDLE_LOCALES: usize = 32;
/// Mirror addresses of one catalog entry.
pub const MAX_WEB_BUNDLE_URLS: usize = 8;
/// Bytes of one catalog address.
pub const MAX_WEB_BUNDLE_URL_BYTES: usize = 2048;

/// File types a bundle may hold, by lowercase extension, with the content
/// type hosts serve them with. `html` is the entry only; no `wasm` (D-06).
pub const WEB_BUNDLE_FILE_TYPES: &[(&str, &str)] = &[
    ("html", "text/html; charset=utf-8"),
    ("js", "text/javascript; charset=utf-8"),
    ("css", "text/css; charset=utf-8"),
    ("json", "application/json"),
    ("woff2", "font/woff2"),
    ("png", "image/png"),
    ("svg", "image/svg+xml"),
    ("webp", "image/webp"),
];

/// A web bundle of a plugin version: in the manifest
/// (`clientExtensions.webBundles`) its identity, digest, size and bridge;
/// in the client catalog also where to fetch it and what the tenant granted
/// (`urls`, `files`, `permissions`, set by Core only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WebBundleDecl {
    /// `[a-z0-9][a-z0-9-]{0,63}`, what [`crate::ui::UiBundleRef::id`] names.
    pub id: String,
    /// SHA-256 of the `.fwb` file, 64 lowercase hex digits.
    #[schemars(regex(pattern = r"^[0-9a-f]{64}$"))]
    pub sha256: String,
    /// Bytes of the `.fwb` file, at most [`MAX_WEB_BUNDLE_BYTES`].
    pub size: u64,
    /// The bridge version the bundle speaks at least, `1..=BRIDGE_VERSION`;
    /// equal to `bundle.json`'s.
    pub bridge: u32,
    /// The bundle sells digital goods or services: Core never selects it for
    /// a client under App Store rules (no in-app purchase capability yet).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub digital_goods: bool,
    /// Catalog only: `https` addresses of the `.fwb` file on the tenant's
    /// mirrors, in failover order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = MAX_WEB_BUNDLE_URLS))]
    pub urls: Vec<String>,
    /// Catalog only, Web and Tauri: `https` base address (ending in `/`) of
    /// the unpacked files, a file at `<files><path>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<String>,
    /// Catalog only: the bridge permissions the tenant approved for this
    /// bundle. Absent grants none.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub permissions: BTreeSet<BridgePermission>,
}

impl WebBundleDecl {
    /// What a manifest may declare: a valid id, a hex digest, a size within
    /// [`MAX_WEB_BUNDLE_BYTES`], a bridge this protocol knows, and none of
    /// the catalog fields.
    pub fn validate_declared(&self) -> Result<(), BundleError> {
        if !crate::ui::valid_bundle_id(&self.id) {
            return Err(BundleError::InvalidDeclaration("id"));
        }
        if !is_sha256_hex(&self.sha256) {
            return Err(BundleError::InvalidDeclaration("sha256"));
        }
        if self.size == 0 || self.size > MAX_WEB_BUNDLE_BYTES {
            return Err(BundleError::InvalidDeclaration("size"));
        }
        if !(1..=BRIDGE_VERSION).contains(&self.bridge) {
            return Err(BundleError::InvalidDeclaration("bridge"));
        }
        if !self.urls.is_empty() || self.files.is_some() || !self.permissions.is_empty() {
            return Err(BundleError::InvalidDeclaration(
                "catalog-only field in a manifest",
            ));
        }
        Ok(())
    }

    /// What a client accepts from the catalog: the declared form with at
    /// least one and at most [`MAX_WEB_BUNDLE_URLS`] `https` addresses and,
    /// when present, an `https` files base ending in `/`. The bridge may be
    /// above this protocol's (Core matched it with the host's).
    pub fn validate_catalog(&self) -> Result<(), BundleError> {
        if !crate::ui::valid_bundle_id(&self.id)
            || !is_sha256_hex(&self.sha256)
            || self.size == 0
            || self.size > MAX_WEB_BUNDLE_BYTES
            || self.bridge == 0
        {
            return Err(BundleError::InvalidDeclaration("catalog entry"));
        }
        if self.urls.is_empty()
            || self.urls.len() > MAX_WEB_BUNDLE_URLS
            || !self.urls.iter().all(|url| is_https_url(url))
        {
            return Err(BundleError::InvalidDeclaration("urls"));
        }
        if self
            .files
            .as_deref()
            .is_some_and(|files| !is_https_url(files) || !files.ends_with('/'))
        {
            return Err(BundleError::InvalidDeclaration("files"));
        }
        Ok(())
    }

    /// The declaration as the manifest holds it: without the catalog fields.
    pub fn declared(&self) -> Self {
        Self {
            urls: Vec::new(),
            files: None,
            permissions: BTreeSet::new(),
            ..self.clone()
        }
    }
}

/// `https://` followed by a host, at most [`MAX_WEB_BUNDLE_URL_BYTES`], no
/// credentials, whitespace or control characters.
pub fn is_https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    url.len() <= MAX_WEB_BUNDLE_URL_BYTES
        && !authority.is_empty()
        && !authority.contains('@')
        && url.bytes().all(|b| b.is_ascii_graphic() && b != b'\\')
}

/// 64 lowercase hex digits.
pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The SHA-256 of `bytes` as [`WebBundleDecl::sha256`] and
/// [`BundleFile::sha256`] hold it.
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// `bundle.json`: what the bundle opens, the bridge it speaks and what of the
/// bridge and the plugin it asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleManifest {
    /// The declaration's id.
    pub id: String,
    /// The entry document, the only HTML file (default `index.html`).
    #[serde(default = "default_entry")]
    pub entry: String,
    /// The bridge version it speaks at least; the declaration's.
    pub bridge: u32,
    /// Bridge permissions it asks for; the tenant approves them.
    #[serde(default)]
    pub permissions: BTreeSet<BridgePermission>,
    /// Wire ids of the plugin operations it calls: user operations of the
    /// manifest, never transient ones.
    #[serde(default)]
    #[schemars(length(max = MAX_BUNDLE_OPERATIONS))]
    pub operations: BTreeSet<String>,
    /// Host capabilities it starts with `host.capability`: a subset of the
    /// manifest's `clientExtensions.nativeCapabilities`.
    #[serde(default)]
    pub capabilities: BTreeSet<NativeCapability>,
    /// Locales it ships texts for (`zh-CN`, `en`).
    #[serde(default)]
    #[schemars(length(max = MAX_BUNDLE_LOCALES))]
    pub locales: Vec<String>,
}

fn default_entry() -> String {
    DEFAULT_BUNDLE_ENTRY.to_owned()
}

impl BundleManifest {
    /// Reads `bundle.json`: at most [`MAX_BUNDLE_MANIFEST_BYTES`], strict
    /// fields, then [`Self::validate`].
    pub fn from_json(bytes: &[u8]) -> Result<Self, BundleError> {
        if bytes.len() > MAX_BUNDLE_MANIFEST_BYTES {
            return Err(BundleError::InvalidBundleManifest(
                "bundle.json is too large",
            ));
        }
        let manifest: Self = serde_json::from_slice::<serde_json::Value>(bytes)
            .and_then(crate::json::from_value)
            .map_err(|_| BundleError::InvalidBundleManifest("invalid bundle.json"))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// The rules that need nothing else: a valid id, an HTML entry at a
    /// bundle path, a bridge this protocol knows, bounded operation and
    /// locale lists of well-formed names.
    pub fn validate(&self) -> Result<(), BundleError> {
        if !crate::ui::valid_bundle_id(&self.id) {
            return Err(BundleError::InvalidBundleManifest("id"));
        }
        if !is_bundle_path(&self.entry) || extension(&self.entry) != Some("html") {
            return Err(BundleError::InvalidBundleManifest("entry"));
        }
        if !(1..=BRIDGE_VERSION).contains(&self.bridge) {
            return Err(BundleError::UnsupportedBridge(self.bridge));
        }
        if self.operations.len() > MAX_BUNDLE_OPERATIONS
            || !self.operations.iter().all(|operation| {
                !operation.is_empty()
                    && operation.len() <= 192
                    && operation
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            })
        {
            return Err(BundleError::InvalidBundleManifest("operations"));
        }
        let mut locales = BTreeSet::new();
        if self.locales.len() > MAX_BUNDLE_LOCALES
            || !self.locales.iter().all(|locale| {
                !locale.is_empty()
                    && locale.len() <= 35
                    && locale
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    && locales.insert(locale.to_ascii_lowercase())
            })
        {
            return Err(BundleError::InvalidBundleManifest("locales"));
        }
        Ok(())
    }

    /// [`Self::validate`], and the rules against the declaration and the
    /// plugin manifest: the declaration's id and bridge, operations that are
    /// user operations of the manifest and not transient, capabilities the
    /// manifest declares as `nativeCapabilities`.
    pub fn validate_for(
        &self,
        decl: &WebBundleDecl,
        manifest: &PluginManifest,
    ) -> Result<(), BundleError> {
        self.validate()?;
        if self.id != decl.id {
            return Err(BundleError::DeclarationMismatch("id"));
        }
        if self.bridge != decl.bridge {
            return Err(BundleError::DeclarationMismatch("bridge"));
        }
        // A staff presentation receives only its authorized page model. All staff writes
        // stay in the console's audited action path, with its permission and MFA checks.
        if manifest
            .admin
            .as_ref()
            .is_some_and(|admin| admin.page_bundles.values().any(|id| id == &self.id))
            && !(self.operations.is_empty()
                && self.permissions.is_empty()
                && self.capabilities.is_empty())
        {
            return Err(BundleError::InvalidBundleManifest(
                "staff pages cannot request app bridge permissions, operations or native capabilities",
            ));
        }
        let user_operations = manifest.user_operation_ids();
        for operation in &self.operations {
            if !user_operations.contains(operation) {
                return Err(BundleError::UndeclaredOperation(operation.clone()));
            }
            if manifest.operation(operation).is_some_and(|op| op.transient) {
                return Err(BundleError::TransientOperation(operation.clone()));
            }
        }
        for capability in &self.capabilities {
            if !manifest
                .client_extensions
                .native_capabilities
                .contains(capability)
            {
                return Err(BundleError::UndeclaredCapability(
                    capability.name().to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// One file of a checked bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleFile {
    /// Bundle path ([`is_bundle_path`]).
    pub path: String,
    pub size: u64,
    /// 64 lowercase hex digits.
    pub sha256: String,
    /// What hosts serve it as ([`content_type`]).
    pub content_type: String,
}

/// What [`check_archive`] found: the bundle manifest, the file list the
/// platform publishes next to the unpacked copy, and the findings shown to
/// the reviewer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleIndex {
    pub manifest: BundleManifest,
    /// The archive's digest and size (the declaration's).
    pub sha256: String,
    pub size: u64,
    /// The sum of the file sizes.
    pub unpacked_size: u64,
    /// Every entry, `bundle.json` included, sorted by path.
    pub files: Vec<BundleFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<BundleWarning>,
}

impl BundleIndex {
    pub fn file(&self, path: &str) -> Option<&BundleFile> {
        self.files
            .binary_search_by(|file| file.path.as_str().cmp(path))
            .ok()
            .map(|index| &self.files[index])
    }
}

/// A finding that does not refuse the bundle (the CSP stops it from
/// running) but that the reviewer sees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleWarning {
    pub path: String,
    pub code: BundleWarningCode,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BundleWarningCode {
    /// `eval(`.
    Eval,
    /// `new Function(`.
    NewFunction,
    /// `import(` of an absolute or protocol-relative address.
    ExternalImport,
}

/// Why a bundle, its declaration or its `bundle.json` is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BundleError {
    #[error("invalid web bundle declaration: {0}")]
    InvalidDeclaration(&'static str),
    #[error("web bundle exceeds 10 MiB")]
    TooLarge,
    #[error("web bundle size differs from the declaration")]
    SizeMismatch,
    #[error("web bundle digest differs from the declaration")]
    DigestMismatch,
    #[error("invalid web bundle archive: {0}")]
    InvalidArchive(&'static str),
    #[error("web bundle has more than 512 files")]
    TooManyFiles,
    #[error("web bundle file exceeds 4 MiB: {0}")]
    FileTooLarge(String),
    #[error("web bundle files exceed 20 MiB")]
    UnpackedTooLarge,
    #[error("invalid web bundle path: {0}")]
    InvalidPath(String),
    #[error("web bundle file type not allowed: {0}")]
    FileType(String),
    #[error("missing web bundle file: {0}")]
    MissingFile(String),
    #[error("invalid bundle.json: {0}")]
    InvalidBundleManifest(&'static str),
    #[error("unsupported bridge version {0}")]
    UnsupportedBridge(u32),
    #[error("bundle.json differs from the declaration: {0}")]
    DeclarationMismatch(&'static str),
    #[error("bundle operation is not a user operation of the manifest: {0}")]
    UndeclaredOperation(String),
    #[error("bundle operation is transient: {0}")]
    TransientOperation(String),
    #[error("bundle capability is not declared by the manifest: {0}")]
    UndeclaredCapability(String),
    #[error("entry document refused: {0:?}")]
    Html(HtmlViolation),
}

impl BundleError {
    /// A stable snake_case code for APIs and the review console.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidDeclaration(_) => "invalid_declaration",
            Self::TooLarge => "too_large",
            Self::SizeMismatch => "size_mismatch",
            Self::DigestMismatch => "digest_mismatch",
            Self::InvalidArchive(_) => "invalid_archive",
            Self::TooManyFiles => "too_many_files",
            Self::FileTooLarge(_) => "file_too_large",
            Self::UnpackedTooLarge => "unpacked_too_large",
            Self::InvalidPath(_) => "invalid_path",
            Self::FileType(_) => "file_type",
            Self::MissingFile(_) => "missing_file",
            Self::InvalidBundleManifest(_) => "invalid_bundle_manifest",
            Self::UnsupportedBridge(_) => "unsupported_bridge",
            Self::DeclarationMismatch(_) => "declaration_mismatch",
            Self::UndeclaredOperation(_) => "undeclared_operation",
            Self::TransientOperation(_) => "transient_operation",
            Self::UndeclaredCapability(_) => "undeclared_capability",
            Self::Html(_) => "unsafe_entry_html",
        }
    }
}

/// A bundle path: relative, at most [`MAX_WEB_BUNDLE_PATH_BYTES`], only
/// `[a-z0-9._/-]`, no empty, `.` or `..` segment, no segment starting with
/// `.`.
pub fn is_bundle_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_WEB_BUNDLE_PATH_BYTES
        && path
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._/-".contains(&b))
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && !segment.starts_with('.'))
}

/// The lowercase extension of a path's last segment.
fn extension(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?;
    let (stem, extension) = name.rsplit_once('.')?;
    (!stem.is_empty()).then_some(extension)
}

/// The content type hosts serve a bundle path with; `None` for a type the
/// bundle may not hold.
pub fn content_type(path: &str) -> Option<&'static str> {
    let extension = extension(path)?;
    WEB_BUNDLE_FILE_TYPES
        .iter()
        .find(|(allowed, _)| *allowed == extension)
        .map(|(_, content_type)| *content_type)
}

/// Whether `path` may be a file of a bundle whose entry is `entry`:
/// `bundle.json`, the entry, or a bundle path of an allowed type other than
/// HTML.
pub fn is_allowed_file(path: &str, entry: &str) -> Result<(), BundleError> {
    if !is_bundle_path(path) {
        return Err(BundleError::InvalidPath(path.to_owned()));
    }
    if path == BUNDLE_MANIFEST_ENTRY || path == entry {
        return Ok(());
    }
    match extension(path) {
        Some("html") | None => Err(BundleError::FileType(path.to_owned())),
        Some(_) if content_type(path).is_some() => Ok(()),
        Some(_) => Err(BundleError::FileType(path.to_owned())),
    }
}

/// Why the entry document is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "rule", content = "detail")]
pub enum HtmlViolation {
    /// Not UTF-8, or a tag, comment or raw-text element left open.
    Malformed,
    /// A `<script>` with content or without `src`.
    InlineScript,
    /// A `<script src>` that is not a relative bundle address.
    ExternalScript(String),
    /// A `<link href>` that is not a relative bundle address.
    ExternalLink(String),
    /// An `on*` attribute.
    EventAttribute(String),
    /// A `javascript:` or `vbscript:` address in an attribute.
    ScriptUrl(String),
    /// `<base>`, `<meta http-equiv>`, `<iframe>`, `<frame>`, `<object>` or
    /// `<embed>`.
    ForbiddenElement(String),
}

/// The static rules of the entry document: no inline script, no inline event
/// attribute, no `javascript:` address, no `<script src>` or `<link href>`
/// outside the bundle, no `<base>`, no `<meta http-equiv>`, no nested
/// documents or plugins. A bundle address is relative: no scheme, not
/// starting with `/` or `\`.
pub fn check_entry_html(html: &[u8]) -> Result<(), HtmlViolation> {
    let html = std::str::from_utf8(html).map_err(|_| HtmlViolation::Malformed)?;
    let bytes = html.as_bytes();
    let mut at = 0;
    while let Some(offset) = html[at..].find('<') {
        at += offset;
        let rest = &html[at..];
        if rest.starts_with("<!--") {
            at += rest.find("-->").ok_or(HtmlViolation::Malformed)? + 3;
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") || rest.starts_with("</") {
            at += rest.find('>').ok_or(HtmlViolation::Malformed)? + 1;
            continue;
        }
        let name_len = rest[1..]
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b':')
            .count();
        if name_len == 0 {
            // A literal `<` in text.
            at += 1;
            continue;
        }
        let name = rest[1..=name_len].to_ascii_lowercase();
        at += 1 + name_len;
        let mut attributes: Vec<(String, String)> = Vec::new();
        loop {
            while at < bytes.len() && (bytes[at].is_ascii_whitespace() || bytes[at] == b'/') {
                at += 1;
            }
            match bytes.get(at) {
                None => return Err(HtmlViolation::Malformed),
                Some(b'>') => {
                    at += 1;
                    break;
                }
                Some(_) => {}
            }
            let start = at;
            while at < bytes.len()
                && !bytes[at].is_ascii_whitespace()
                && !matches!(bytes[at], b'=' | b'>' | b'/')
            {
                at += 1;
            }
            let attribute = html[start..at].to_ascii_lowercase();
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            let mut value = String::new();
            if bytes.get(at) == Some(&b'=') {
                at += 1;
                while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                    at += 1;
                }
                match bytes.get(at) {
                    Some(&quote) if quote == b'"' || quote == b'\'' => {
                        let end = html[at + 1..]
                            .find(quote as char)
                            .ok_or(HtmlViolation::Malformed)?;
                        value = html[at + 1..at + 1 + end].to_owned();
                        at += end + 2;
                    }
                    Some(_) => {
                        let start = at;
                        while at < bytes.len()
                            && !bytes[at].is_ascii_whitespace()
                            && bytes[at] != b'>'
                        {
                            at += 1;
                        }
                        value = html[start..at].to_owned();
                    }
                    None => return Err(HtmlViolation::Malformed),
                }
            }
            if attribute.is_empty() {
                // A stray `=` and its value.
                continue;
            }
            attributes.push((attribute, value));
        }
        check_tag(&name, &attributes)?;
        if matches!(
            name.as_str(),
            "script" | "style" | "textarea" | "title" | "xmp" | "noscript"
        ) {
            let lower = html[at..].to_ascii_lowercase();
            let close = lower
                .find(&format!("</{name}"))
                .ok_or(HtmlViolation::Malformed)?;
            if name == "script" && !html[at..at + close].trim().is_empty() {
                return Err(HtmlViolation::InlineScript);
            }
            at += close;
        }
    }
    Ok(())
}

fn check_tag(name: &str, attributes: &[(String, String)]) -> Result<(), HtmlViolation> {
    let attribute = |wanted: &str| {
        attributes
            .iter()
            .find(|(attribute, _)| attribute == wanted)
            .map(|(_, value)| value.as_str())
    };
    match name {
        "base" | "iframe" | "frame" | "frameset" | "object" | "embed" | "portal" => {
            return Err(HtmlViolation::ForbiddenElement(name.to_owned()));
        }
        "meta" if attribute("http-equiv").is_some() => {
            return Err(HtmlViolation::ForbiddenElement(
                "meta http-equiv".to_owned(),
            ));
        }
        _ => {}
    }
    for (attribute, value) in attributes {
        if attribute.len() > 2 && attribute.starts_with("on") {
            return Err(HtmlViolation::EventAttribute(attribute.clone()));
        }
        let normalized = normalize_url(value);
        if normalized.contains("javascript:") || normalized.contains("vbscript:") {
            return Err(HtmlViolation::ScriptUrl(attribute.clone()));
        }
    }
    match name {
        "script" => match attribute("src") {
            Some(src) if is_relative_url(src) => Ok(()),
            Some(src) => Err(HtmlViolation::ExternalScript(src.to_owned())),
            None => Err(HtmlViolation::InlineScript),
        },
        "link" => match attribute("href") {
            Some(href) if !is_relative_url(href) => {
                Err(HtmlViolation::ExternalLink(href.to_owned()))
            }
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

/// An attribute value as a browser reads an address in it: numeric and the
/// `colon`, `tab` and `newline` character references decoded, ASCII
/// whitespace and control characters dropped, lowercased.
fn normalize_url(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = rest.find('&') {
        decoded.push_str(&rest[..index]);
        rest = &rest[index..];
        let end = rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .map_or(rest.len(), |end| end + 1);
        let reference = rest[1..end].to_ascii_lowercase();
        let character = if let Some(hex) = reference.strip_prefix("#x") {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else if let Some(decimal) = reference.strip_prefix('#') {
            decimal.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            match reference.as_str() {
                "colon" => Some(':'),
                "tab" => Some('\t'),
                "newline" => Some('\n'),
                _ => None,
            }
        };
        match character {
            Some(character) => {
                decoded.push(character);
                rest = rest[end..].strip_prefix(';').unwrap_or(&rest[end..]);
            }
            None => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && !c.is_control())
        .flat_map(char::to_lowercase)
        .collect()
}

/// A relative address inside the bundle: no scheme, not rooted (`/`, `\`).
fn is_relative_url(value: &str) -> bool {
    let normalized = normalize_url(value);
    let before_path = normalized.split(['/', '?', '#']).next().unwrap_or_default();
    !normalized.is_empty()
        && !normalized.starts_with('/')
        && !normalized.starts_with('\\')
        && !before_path.contains(':')
}

/// What the reviewer is told about a script: `eval(`, `new Function(` and
/// `import(` of an absolute or protocol-relative address, each once.
pub fn scan_script(text: &str) -> Vec<BundleWarningCode> {
    let compact = compact_script(text);
    let mut found = BTreeSet::new();
    if calls(&compact, "eval(") {
        found.insert(BundleWarningCode::Eval);
    }
    if calls(&compact, "new Function(") {
        found.insert(BundleWarningCode::NewFunction);
    }
    for quote in ['"', '\'', '`'] {
        for prefix in ["http:", "https:", "//"] {
            if calls(&compact, &format!("import({quote}{prefix}")) {
                found.insert(BundleWarningCode::ExternalImport);
            }
        }
    }
    found.into_iter().collect()
}

/// The script with whitespace dropped, except one space where it separates
/// two identifier characters: `await import (` stays two words (`await
/// import(`) and `new Function (` reads `new Function(`.
fn compact_script(text: &str) -> String {
    let identifier = |c: char| c.is_alphanumeric() || matches!(c, '_' | '$');
    let mut compact = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && identifier(c) && compact.chars().next_back().is_some_and(identifier) {
            compact.push(' ');
        }
        pending_space = false;
        compact.push(c);
    }
    compact
}

/// Whether `needle` occurs in `text` not as the tail of a longer identifier
/// or a property (`retrieval(`, `x.eval(`).
fn calls(text: &str, needle: &str) -> bool {
    text.match_indices(needle).any(|(index, _)| {
        text[..index]
            .chars()
            .next_back()
            .is_none_or(|before| !(before.is_alphanumeric() || matches!(before, '_' | '$' | '.')))
    })
}

#[cfg(feature = "bundle-archive")]
pub use archive::{UnpackedBundle, check_archive, unpack_archive, write_archive};

#[cfg(feature = "bundle-archive")]
mod archive {
    use std::collections::BTreeMap;
    use std::io::{Cursor, Read, Write};

    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, DateTime, System, ZipArchive, ZipWriter};

    use super::*;

    /// A bundle read by [`unpack_archive`]: its manifest, its file list and
    /// the file contents by path.
    #[derive(Debug, Clone)]
    pub struct UnpackedBundle {
        pub manifest: BundleManifest,
        /// Every entry, `bundle.json` included, sorted by path.
        pub index: Vec<BundleFile>,
        pub files: BTreeMap<String, Vec<u8>>,
        pub unpacked_size: u64,
    }

    /// Reads a bundle whose declaration says `sha256` and `size`: the size
    /// and the digest first, then the zip (stored plain files only, no
    /// duplicate or inconsistent names), every path and type against the
    /// allowlists, the limits of D-05, and `bundle.json`
    /// ([`BundleManifest::from_json`]) with its entry present. What the
    /// client cache runs before writing files; [`check_archive`] adds the
    /// rules that need the plugin manifest.
    pub fn unpack_archive(
        bytes: &[u8],
        sha256: &str,
        size: u64,
    ) -> Result<UnpackedBundle, BundleError> {
        if bytes.len() as u64 > MAX_WEB_BUNDLE_BYTES {
            return Err(BundleError::TooLarge);
        }
        if bytes.len() as u64 != size {
            return Err(BundleError::SizeMismatch);
        }
        if sha256_hex(bytes) != sha256 {
            return Err(BundleError::DigestMismatch);
        }
        let mut archive = ZipArchive::new(Cursor::new(bytes))
            .map_err(|_| BundleError::InvalidArchive("not a zip archive"))?;
        if archive.len() > MAX_WEB_BUNDLE_FILES {
            return Err(BundleError::TooManyFiles);
        }
        if archive.has_duplicate_names() {
            return Err(BundleError::InvalidArchive("duplicate entry"));
        }
        archive
            .validate_header_consistency()
            .map_err(|_| BundleError::InvalidArchive("inconsistent zip headers"))?;
        let mut files = BTreeMap::new();
        let mut remaining = MAX_WEB_BUNDLE_UNPACKED_BYTES;
        for index in 0..archive.len() {
            let entry = archive
                .by_index(index)
                .map_err(|_| BundleError::InvalidArchive("unreadable entry"))?;
            let name = entry
                .name()
                .map_err(|_| BundleError::InvalidArchive("unreadable entry name"))?
                .into_owned();
            if !is_bundle_path(&name) {
                return Err(BundleError::InvalidPath(name));
            }
            if entry.is_dir()
                || entry.is_symlink()
                || entry.encrypted()
                || entry.compression() != CompressionMethod::Stored
            {
                return Err(BundleError::InvalidArchive(
                    "entries are plain stored files",
                ));
            }
            if entry.size() > MAX_WEB_BUNDLE_FILE_BYTES {
                return Err(BundleError::FileTooLarge(name));
            }
            if entry.size() > remaining {
                return Err(BundleError::UnpackedTooLarge);
            }
            let mut content = Vec::new();
            entry
                .take(MAX_WEB_BUNDLE_FILE_BYTES.min(remaining).saturating_add(1))
                .read_to_end(&mut content)
                .map_err(|_| BundleError::InvalidArchive("unreadable entry"))?;
            if content.len() as u64 > MAX_WEB_BUNDLE_FILE_BYTES {
                return Err(BundleError::FileTooLarge(name));
            }
            remaining = remaining
                .checked_sub(content.len() as u64)
                .ok_or(BundleError::UnpackedTooLarge)?;
            if files.insert(name, content).is_some() {
                return Err(BundleError::InvalidArchive("duplicate entry"));
            }
        }
        let manifest = BundleManifest::from_json(
            files
                .get(BUNDLE_MANIFEST_ENTRY)
                .ok_or_else(|| BundleError::MissingFile(BUNDLE_MANIFEST_ENTRY.to_owned()))?,
        )?;
        if !files.contains_key(&manifest.entry) {
            return Err(BundleError::MissingFile(manifest.entry.clone()));
        }
        for path in files.keys() {
            is_allowed_file(path, &manifest.entry)?;
        }
        let index = files
            .iter()
            .map(|(path, content)| BundleFile {
                path: path.clone(),
                size: content.len() as u64,
                sha256: sha256_hex(content),
                content_type: content_type(path).unwrap_or("application/json").to_owned(),
            })
            .collect();
        Ok(UnpackedBundle {
            manifest,
            index,
            files,
            unpacked_size: MAX_WEB_BUNDLE_UNPACKED_BYTES - remaining,
        })
    }

    /// The platform's check of an uploaded bundle (design 8.2), run when
    /// the upload is read back: [`unpack_archive`] against the declaration,
    /// `bundle.json` against the declaration and the plugin manifest
    /// ([`BundleManifest::validate_for`]), and the entry document
    /// ([`check_entry_html`]). Scripts are scanned into warnings.
    pub fn check_archive(
        bytes: &[u8],
        decl: &WebBundleDecl,
        manifest: &PluginManifest,
    ) -> Result<BundleIndex, BundleError> {
        decl.declared().validate_declared()?;
        let unpacked = unpack_archive(bytes, &decl.sha256, decl.size)?;
        unpacked.manifest.validate_for(decl, manifest)?;
        check_entry_html(&unpacked.files[&unpacked.manifest.entry]).map_err(BundleError::Html)?;
        let mut warnings = Vec::new();
        for (path, content) in &unpacked.files {
            if extension(path) == Some("js") {
                let text = String::from_utf8_lossy(content);
                warnings.extend(scan_script(&text).into_iter().map(|code| BundleWarning {
                    path: path.clone(),
                    code,
                }));
            }
        }
        Ok(BundleIndex {
            manifest: unpacked.manifest,
            sha256: decl.sha256.clone(),
            size: decl.size,
            unpacked_size: unpacked.unpacked_size,
            files: unpacked.index,
            warnings,
        })
    }

    /// Builds a bundle from `bundle.json` and the other files by path. The
    /// same contents always give the same bytes: sorted entries, stored,
    /// with a fixed timestamp, mode and system. The result passes
    /// [`unpack_archive`] with its own digest and size.
    pub fn write_archive(
        manifest: &BundleManifest,
        files: &BTreeMap<String, Vec<u8>>,
    ) -> Result<Vec<u8>, BundleError> {
        manifest.validate()?;
        if files.contains_key(BUNDLE_MANIFEST_ENTRY) {
            return Err(BundleError::InvalidArchive(
                "bundle.json is written from the manifest",
            ));
        }
        if !files.contains_key(&manifest.entry) {
            return Err(BundleError::MissingFile(manifest.entry.clone()));
        }
        if files.len() + 1 > MAX_WEB_BUNDLE_FILES {
            return Err(BundleError::TooManyFiles);
        }
        let manifest_json = serde_json::to_vec_pretty(manifest)
            .map_err(|_| BundleError::InvalidBundleManifest("unwritable bundle.json"))?;
        let mut entries: BTreeMap<&str, &[u8]> = BTreeMap::new();
        let mut total = manifest_json.len() as u64;
        for (path, content) in files {
            is_allowed_file(path, &manifest.entry)?;
            if content.len() as u64 > MAX_WEB_BUNDLE_FILE_BYTES {
                return Err(BundleError::FileTooLarge(path.clone()));
            }
            total += content.len() as u64;
            entries.insert(path.as_str(), content.as_slice());
        }
        if total > MAX_WEB_BUNDLE_UNPACKED_BYTES {
            return Err(BundleError::UnpackedTooLarge);
        }
        entries.insert(BUNDLE_MANIFEST_ENTRY, manifest_json.as_slice());
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .last_modified_time(DateTime::DEFAULT)
            .unix_permissions(0o644)
            .system(System::Unix);
        let failed = |_| BundleError::InvalidArchive("bundle could not be written");
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in entries {
            writer.start_file(name, options).map_err(failed)?;
            writer
                .write_all(content)
                .map_err(|_| BundleError::InvalidArchive("bundle could not be written"))?;
        }
        let bytes = writer.finish().map_err(failed)?.into_inner();
        if bytes.len() as u64 > MAX_WEB_BUNDLE_BYTES {
            return Err(BundleError::TooLarge);
        }
        Ok(bytes)
    }
}
