use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CmdexConfig {
    pub agents: Vec<AgentDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDefinition {
    pub name: String,
    pub workspace: PathBuf,
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct RawConfig {
    #[serde(default)]
    agents: Vec<RawAgentDefinition>,
}

#[derive(Debug, Deserialize, Serialize)]
struct RawAgentDefinition {
    name: String,
    workspace: String,
}

pub struct ConfigStore;

impl ConfigStore {
    pub fn default_path() -> Result<PathBuf> {
        let home = Self::home_dir()?;
        Ok(home.join(".cmdex.yml"))
    }

    pub fn load(path: &Path) -> Result<CmdexConfig> {
        if !path.exists() {
            return Ok(CmdexConfig::default());
        }

        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let raw: RawConfig = serde_yaml::from_str(&contents)
            .with_context(|| format!("failed to parse YAML at {}", path.display()))?;

        let agents = raw
            .agents
            .into_iter()
            .map(|raw_agent| {
                Ok(AgentDefinition {
                    name: raw_agent.name.trim().to_string(),
                    workspace: Self::normalize_path(&raw_agent.workspace)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(CmdexConfig { agents })
    }

    pub fn save(path: &Path, config: &CmdexConfig) -> Result<()> {
        let raw = RawConfig {
            agents: config
                .agents
                .iter()
                .map(|agent| RawAgentDefinition {
                    name: agent.name.clone(),
                    workspace: Self::compact_home(&agent.workspace),
                })
                .collect(),
        };

        let yaml = serde_yaml::to_string(&raw).context("failed to serialize config")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }
        fs::write(path, yaml).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn validate_agent_input(name: &str, workspace: &str) -> Result<AgentDefinition> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("Agent name cannot be empty."));
        }

        let workspace = Self::normalize_path(workspace)?;
        if !workspace.exists() {
            return Err(anyhow!("Workspace does not exist."));
        }
        if !workspace.is_dir() {
            return Err(anyhow!("Workspace must be a directory."));
        }

        Ok(AgentDefinition {
            name: name.to_string(),
            workspace,
        })
    }

    pub fn compact_home(path: &Path) -> String {
        match Self::home_dir() {
            Ok(home) if path.starts_with(&home) => {
                if let Ok(rest) = path.strip_prefix(&home) {
                    if rest.as_os_str().is_empty() {
                        "~".to_string()
                    } else {
                        format!("~/{}", rest.display())
                    }
                } else {
                    path.display().to_string()
                }
            }
            _ => path.display().to_string(),
        }
    }

    fn normalize_path(input: &str) -> Result<PathBuf> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("Workspace cannot be empty."));
        }

        let expanded = if trimmed == "~" {
            Self::home_dir()?
        } else if let Some(rest) = trimmed.strip_prefix("~/") {
            Self::home_dir()?.join(rest)
        } else {
            PathBuf::from(trimmed)
        };

        if expanded.is_absolute() {
            Ok(expanded)
        } else {
            Ok(env::current_dir()
                .context("failed to resolve current directory")?
                .join(expanded))
        }
    }

    fn home_dir() -> Result<PathBuf> {
        env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("HOME is not set"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compacts_home_paths() {
        let home = ConfigStore::home_dir().expect("home");
        let nested = home.join("projects/example");
        assert_eq!(ConfigStore::compact_home(&nested), "~/projects/example");
    }

    #[test]
    fn parses_yaml_with_tilde_workspace() {
        let yaml = r#"
agents:
  - name: demo
    workspace: ~/projects/demo
"#;
        let raw: RawConfig = serde_yaml::from_str(yaml).expect("raw config");
        let agent = AgentDefinition {
            name: raw.agents[0].name.clone(),
            workspace: ConfigStore::normalize_path(&raw.agents[0].workspace).expect("normalized"),
        };

        assert_eq!(agent.name, "demo");
        assert!(agent.workspace.is_absolute());
    }
}
