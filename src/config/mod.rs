use serde_derive::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path;
use typemap::Key;

mod snippet;
pub mod types;

use crate::config::snippet::load_snippet;
use crate::config::types::keys;
use crate::config::types::Command;
use crate::config::types::CompilerConfig;

const DEFAULT_CONFIG: &str = include_str!("../../assets/default_config.toml");

#[derive(Deserialize, Debug)]
struct ConfigToml {
    file: HashMap<String, LanguageConfigToml>,
    file_default: Option<LanguageConfigToml>,
}

#[derive(Deserialize, Debug)]
struct LanguageConfigToml {
    ansi_color: Option<bool>,
    snippets: Option<Vec<String>>,
    indent_width: Option<usize>,
    lsp: Option<Vec<String>>,
    formatter: Option<Vec<String>>,
    syntax: Option<String>,
    compiler: Option<CompilerConfig>,
}

pub struct LanguageConfig(typemap::TypeMap);

impl Default for LanguageConfig {
    fn default() -> Self {
        Self(typemap::TypeMap::new())
    }
}

impl LanguageConfig {
    fn insert_option<Key: typemap::Key>(&mut self, value: Option<Key::Value>) {
        if let Some(value) = value {
            self.0.insert::<Key>(value);
        }
    }
}

#[derive(Default)]
struct Config {
    file: HashMap<OsString, LanguageConfig>,
    file_default: Option<LanguageConfig>,
}

pub struct ConfigWithDefault {
    default: Config,
    config: Config,
}

impl Into<LanguageConfig> for LanguageConfigToml {
    fn into(self) -> LanguageConfig {
        let snippets = self
            .snippets
            .unwrap_or_default()
            .iter()
            .map(path::PathBuf::from)
            .filter_map(|p| load_snippet(p).ok())
            .fold(BTreeMap::new(), |mut a, mut b| {
                a.append(&mut b);
                a
            });

        let mut language_config = LanguageConfig::default();

        language_config.insert_option::<keys::ANSIColor>(self.ansi_color);
        language_config.0.insert::<keys::Snippets>(snippets);
        language_config.insert_option::<keys::IndentWidth>(self.indent_width);
        language_config.insert_option::<keys::LSP>(
            self.lsp.as_ref().map(Vec::as_slice).and_then(Command::new),
        );
        language_config.insert_option::<keys::Formatter>(
            self.formatter
                .as_ref()
                .map(Vec::as_slice)
                .and_then(Command::new),
        );
        language_config.insert_option::<keys::SyntaxExtension>(self.syntax);
        language_config.insert_option::<keys::Compiler>(self.compiler);

        language_config
    }
}

impl Into<Config> for ConfigToml {
    fn into(self) -> Config {
        Config {
            file: self
                .file
                .into_iter()
                .map(|(k, v)| (OsString::from(k), v.into()))
                .collect(),
            file_default: self.file_default.map(Into::into),
        }
    }
}

fn parse_config(s: &str) -> Result<Config, failure::Error> {
    let config_toml: ConfigToml = toml::from_str(&s)?;
    Ok(config_toml.into())
}

pub fn parse_config_with_default(s: &str) -> Result<ConfigWithDefault, failure::Error> {
    let default = toml::from_str::<ConfigToml>(DEFAULT_CONFIG)
        .map(Into::into)
        .unwrap();

    let config = parse_config(s)?;

    Ok(ConfigWithDefault { default, config })
}

impl Default for ConfigWithDefault {
    fn default() -> Self {
        let default = toml::from_str::<ConfigToml>(DEFAULT_CONFIG)
            .map(Into::into)
            .unwrap();

        Self {
            default,
            config: Config::default(),
        }
    }
}

impl Config {
    fn get<A: Key>(&self, extension: Option<&OsStr>) -> Option<&A::Value> {
        if let Some(extension) = extension {
            self.file
                .get(extension)
                .and_then(|config| config.0.get::<A>())
                .or_else(|| {
                    self.file_default
                        .as_ref()
                        .and_then(|config| config.0.get::<A>())
                })
        } else {
            self.file_default
                .as_ref()
                .and_then(|config| config.0.get::<A>())
        }
    }

    fn snippets(&self, extension: Option<&OsStr>) -> BTreeMap<String, String> {
        if let Some(extension) = extension {
            let mut snippets = self
                .file
                .get(extension)
                .and_then(|config| config.0.get::<keys::Snippets>())
                .cloned()
                .unwrap_or_default();
            let mut default_snippets = self
                .file_default
                .as_ref()
                .and_then(|config| config.0.get::<types::keys::Snippets>())
                .cloned()
                .unwrap_or_default();

            snippets.append(&mut default_snippets);
            snippets
        } else {
            self.file_default
                .as_ref()
                .and_then(|config| config.0.get::<types::keys::Snippets>())
                .cloned()
                .unwrap_or_default()
        }
    }
}

impl ConfigWithDefault {
    pub fn get<A: Key>(&self, extension: Option<&OsStr>) -> Option<&A::Value> {
        self.config
            .get::<A>(extension)
            .or_else(|| self.default.get::<A>(extension))
    }

    pub fn snippets(&self, extension: Option<&OsStr>) -> BTreeMap<String, String> {
        self.config.snippets(extension)
    }
}
