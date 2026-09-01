use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    #[serde(default = "default_pull_port")]
    pub pull_port: u16,
}

pub fn default_spool_dir() -> String {
    "~/.cache/ssh-paste".into()
}

pub fn default_shim_dir() -> String {
    "~/.local/bin".into()
}

pub fn default_pull_port() -> u16 {
    7717
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("no config directory on this platform")?;
    Ok(base.join("ssh-paste").join("config.toml"))
}

pub fn load() -> Result<Config> {
    load_from(&config_path()?)
}

pub fn load_from(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

impl Config {
    pub fn save(&self) -> Result<()> {
        self.save_to(&config_path()?)
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        let dir = path.parent().context("config path has no parent")?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        std::fs::write(path, toml::to_string_pretty(self)?)
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
            if dir.ends_with('/') {
                bail!(
                    "{label} must not end with '/' (got '{dir}'); it names a directory ssh-paste owns, not the home or root directory"
                );
            }
        }
        if self.pull_port == 0 {
            bail!("pull_port must be 1-65535");
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

    fn temp_config_path() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("ssh-paste-config-{}-{nanos}", std::process::id()))
            .join("config.toml")
    }

    #[test]
    fn load_from_missing_path_returns_default() {
        let cfg = load_from(&temp_config_path()).unwrap();
        assert!(cfg.default_target.is_none());
        assert!(cfg.targets.is_empty());
    }

    #[test]
    fn save_to_then_load_from_roundtrips() {
        let path = temp_config_path();
        let mut targets = BTreeMap::new();
        targets.insert(
            "pod".to_string(),
            Target {
                host: "hermes-pod".into(),
                spool_dir: "~/.cache/ssh-paste".into(),
                shim_dir: "~/.local/bin".into(),
                pull_port: 7717,
            },
        );
        let cfg = Config {
            default_target: Some("pod".into()),
            targets,
        };

        cfg.save_to(&path).unwrap();
        assert!(path.exists());

        let back = load_from(&path).unwrap();
        assert_eq!(back.default_target.as_deref(), Some("pod"));
        assert_eq!(back.targets["pod"].host, "hermes-pod");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn parses_minimal_target_with_defaults() {
        let cfg = parsed("[targets.pod]\nhost = \"hermes-pod\"\n");
        let t = &cfg.targets["pod"];
        assert_eq!(t.host, "hermes-pod");
        assert_eq!(t.spool_dir, "~/.cache/ssh-paste");
        assert_eq!(t.shim_dir, "~/.local/bin");
        assert_eq!(t.pull_port, 7717);
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
            pull_port: 7717,
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
        for dir in ["~", "~/", "/", "~/x/"] {
            assert!(
                bad("h", dir, "~/.local/bin").validate().is_err(),
                "spool_dir '{dir}' accepted"
            );
            assert!(
                bad("h", "~/.cache/ssh-paste", dir).validate().is_err(),
                "shim_dir '{dir}' accepted"
            );
        }
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
        assert!(
            Target {
                host: "h".into(),
                spool_dir: "~/.cache/ssh-paste".into(),
                shim_dir: "~/.local/bin".into(),
                pull_port: 0,
            }
            .validate()
            .is_err(),
            "pull_port 0 accepted"
        );
    }
}
