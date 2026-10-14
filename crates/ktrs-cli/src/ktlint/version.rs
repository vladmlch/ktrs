//! Which ktlint CLI the drop-in imitates: `--ktlint-version=<1.8|2.0>` (ktrs only, hidden), else
//! `ktrs_ktlint_version` of the `.editorconfig` that applies to the working directory, else 2.0. Chosen once
//! per run and passed to the engine as an `.editorconfig` override, so every file follows it. The engine and
//! rule switches: research/26-ktlint-18-mode.md.

use std::path::Path;

use ktrs_editorconfig::{AnyPropertyType, EnumValue, NoCache, PropertyTypeRegistry, ResourcePropertiesService};
use ktrs_lint::editorconfig::{KTLINT_VERSION_PROPERTY_TYPE, KtlintVersion};
pub use ktrs_lint::editorconfig::with_ktlint_version;

use crate::ktlint::command_line::ExitCode;
use crate::ktlint::reporter::KTLINT_VERSION;

pub const KTLINT_VERSION_OPTION: &str = "--ktlint-version";

/// The `--ktlint-version` value among `tokens` (before `--`; the last one wins), else the working
/// directory's `.editorconfig`. `Err` is a Clikt-style usage error message.
pub fn resolve_ktlint_version(tokens: &[String], working_dir: &Path) -> Result<KtlintVersion, String> {
    match option_value(tokens)? {
        Some(value) => KtlintVersion::value_of(value)
            .ok_or_else(|| format!("invalid value for {KTLINT_VERSION_OPTION}: invalid choice: {value}. (choose from 1.8, 2.0)")),
        None => Ok(working_dir_ktlint_version(working_dir)),
    }
}

fn option_value(tokens: &[String]) -> Result<Option<&str>, String> {
    let mut value: Option<&str> = None;
    let mut tokens = tokens.iter().take_while(|t| *t != "--");
    while let Some(token) = tokens.next() {
        if let Some(attached) = token.strip_prefix(KTLINT_VERSION_OPTION).and_then(|rest| rest.strip_prefix('=')) {
            value = Some(attached);
        } else if token == KTLINT_VERSION_OPTION {
            value = Some(tokens.next().ok_or_else(|| format!("option {KTLINT_VERSION_OPTION} requires a value"))?);
        }
    }
    Ok(value)
}

/// Only `ktrs_ktlint_version`, read with a minimal registry (the standard ec4j types plus this one, so a
/// `root = true` still stops the walk) and `NoCache`: the shared cache is keyed by path alone and must
/// keep the engine's later full-registry parse, and startup need not build the rule providers for this.
/// An unreadable `.editorconfig` counts as unset here; the engine reports it on the first file.
fn working_dir_ktlint_version(working_dir: &Path) -> KtlintVersion {
    let registry = PropertyTypeRegistry::with_defaults([&KTLINT_VERSION_PROPERTY_TYPE as &dyn AnyPropertyType]);
    let service = ResourcePropertiesService {
        cache: &NoCache,
        config_file_name: ".editorconfig",
        default_editor_configs: Vec::new(),
        keep_unset: true,
        registry: &registry,
        root_directories: Vec::new(),
    };
    // `EditorConfigLoader.load`'s `to_absolute_path`, so a relative `working_dir` walks the same chain.
    let resource = working_dir.join(".kt");
    let resource = if resource.is_absolute() {
        resource
    } else {
        std::env::current_dir().unwrap_or_default().join(resource)
    };
    let properties = match service.query_properties(&resource) {
        Ok(resource_properties) => resource_properties.into_properties(),
        Err(_) => return KtlintVersion::default(),
    };
    let source = properties
        .iter()
        .find(|p| p.name() == KTLINT_VERSION_PROPERTY_TYPE.name)
        .and_then(|p| p.source_value());
    KTLINT_VERSION_PROPERTY_TYPE.parse(source).into_parsed().unwrap_or_default()
}

/// The release `--version` and the SARIF reporter show.
pub fn release(version: KtlintVersion) -> &'static str {
    match version {
        KtlintVersion::V1_8 => "1.8.0",
        KtlintVersion::V2_0 => KTLINT_VERSION,
    }
}

/// The JVM package prefix in logger and exception class names.
pub fn package(version: KtlintVersion) -> &'static str {
    match version {
        KtlintVersion::V1_8 => "com.pinterest.ktlint",
        KtlintVersion::V2_0 => "io.github.ktlint.core",
    }
}

/// The GitHub repository named in messages and git hooks.
pub fn repository(version: KtlintVersion) -> &'static str {
    match version {
        KtlintVersion::V1_8 => "https://github.com/pinterest/ktlint",
        KtlintVersion::V2_0 => "https://github.com/ktlint/ktlint",
    }
}

/// `exitKtLintProcess`: 1.8 exits 1 for every failure but an unparsable format result (123).
pub fn exit_value(version: KtlintVersion, code: ExitCode) -> i32 {
    match (version, code) {
        (KtlintVersion::V1_8, ExitCode::Ok | ExitCode::ParseExceptionAfterFormat) | (KtlintVersion::V2_0, _) => code as i32,
        (KtlintVersion::V1_8, _) => 1,
    }
}
