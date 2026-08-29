use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub default_target: Option<String>,
    #[serde(default)]
    pub targets: BTreeMap<String, Target>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Target {
    pub host: String,
    #[serde(default = "default_spool_dir")]
    pub spool_dir: String,
    #[serde(default = "default_shim_dir")]
    pub shim_dir: String,
}

fn default_spool_dir() -> String {
    "~/.cache/ssh-paste".into()
}

fn default_shim_dir() -> String {
    "~/.local/bin".into()
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("no config directory on this platform")?;
    Ok(base.join("ssh-paste").join("config.toml"))
}

pub fn load() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

impl Config {
    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        let dir = path.parent().context("config path has no parent")?;
        std::fs::create_dir_all(dir)?;
        std::fs::write(&path, toml::to_string_pretty(self)?)
            .with_context(|| format!("writing {}", path.display()))
    }

    pub fn resolve(&self, name: Option<&str>) -> Result<(&str, &Target)> {
        let name = match name.or(self.default_target.as_deref()) {
            Some(n) => n,
            None => bail!(
                "no target given and no default_target set; run `ssh-paste setup <ssh-alias>` first"
            ),
        };
        match self.targets.get_key_value(name) {
            Some((k, v)) => Ok((k.as_str(), v)),
            None => bail!(
                "unknown target '{name}'; run `ssh-paste setup <ssh-alias> --name {name}` first"
            ),
        }
    }
}

impl Target {
    pub fn validate(&self) -> Result<()> {
        if self.host.is_empty() {
            bail!("target host is empty");
        }
        if self.host.starts_with('-') {
            bail!("target host may not start with '-'");
        }
        if self
            .host
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
        {
            bail!("target host contains whitespace or control characters");
        }
        for (label, dir) in [("spool_dir", &self.spool_dir), ("shim_dir", &self.shim_dir)] {
            if !(dir.starts_with("~/") || dir.starts_with('/')) {
                bail!("{label} must start with ~/ or / (got '{dir}')");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(toml_str: &str) -> Config {
        toml::from_str(toml_str).unwrap()
    }

    #[test]
    fn parses_minimal_target_with_defaults() {
        let cfg = parsed("[targets.pod]\nhost = \"hermes-pod\"\n");
        let t = &cfg.targets["pod"];
        assert_eq!(t.host, "hermes-pod");
        assert_eq!(t.spool_dir, "~/.cache/ssh-paste");
        assert_eq!(t.shim_dir, "~/.local/bin");
    }

    #[test]
    fn serializes_roundtrip() {
        let cfg = parsed("default_target = \"pod\"\n[targets.pod]\nhost = \"h\"\n");
        let out = toml::to_string_pretty(&cfg).unwrap();
        let back = parsed(&out);
        assert_eq!(back.default_target.as_deref(), Some("pod"));
        assert_eq!(back.targets["pod"].host, "h");
    }

    #[test]
    fn resolve_uses_explicit_then_default() {
        let cfg =
            parsed("default_target = \"a\"\n[targets.a]\nhost=\"x\"\n[targets.b]\nhost=\"y\"\n");
        assert_eq!(cfg.resolve(Some("b")).unwrap().1.host, "y");
        assert_eq!(cfg.resolve(None).unwrap().1.host, "x");
        let err = cfg.resolve(Some("nope")).unwrap_err().to_string();
        assert!(err.contains("ssh-paste setup"), "hint missing: {err}");
    }

    #[test]
    fn resolve_without_default_or_name_fails() {
        let cfg = parsed("[targets.a]\nhost=\"x\"\n");
        assert!(cfg.resolve(None).is_err());
    }

    #[test]
    fn validate_rejects_bad_values() {
        let bad = |host: &str, spool: &str, shim: &str| Target {
            host: host.into(),
            spool_dir: spool.into(),
            shim_dir: shim.into(),
        };
        assert!(
            bad("-oProxyCommand=evil", "~/.cache/ssh-paste", "~/.local/bin")
                .validate()
                .is_err()
        );
        assert!(
            bad("", "~/.cache/ssh-paste", "~/.local/bin")
                .validate()
                .is_err()
        );
        assert!(
            bad("h\nx", "~/.cache/ssh-paste", "~/.local/bin")
                .validate()
                .is_err()
        );
        assert!(
            bad("h", "relative/path", "~/.local/bin")
                .validate()
                .is_err()
        );
        assert!(bad("h", "~/.cache/ssh-paste", "bin").validate().is_err());
        assert!(
            bad("h", "~/.cache/ssh-paste", "~/.local/bin")
                .validate()
                .is_ok()
        );
        assert!(
            bad("user@host", "/var/tmp/spool", "/opt/bin")
                .validate()
                .is_ok()
        );
    }
}
