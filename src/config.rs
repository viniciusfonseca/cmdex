use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CmdexConfig {
    pub agents: Vec<AgentDefinition>,
    pub lsp: LspConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDefinition {
    pub name: String,
    pub workspace: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LspConfig {
    pub servers: Vec<LspServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspServerConfig {
    pub name: String,
    pub language_id: String,
    pub extensions: Vec<String>,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct RawConfig {
    #[serde(default)]
    agents: Vec<RawAgentDefinition>,
    #[serde(default, skip_serializing_if = "RawLspConfig::is_empty")]
    lsp: RawLspConfig,
}

#[derive(Debug, Deserialize, Serialize)]
struct RawAgentDefinition {
    name: String,
    workspace: String,
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct RawLspConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    servers: Vec<RawLspServerConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
struct RawLspServerConfig {
    name: String,
    language_id: String,
    #[serde(default)]
    extensions: Vec<String>,
    command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<String, String>,
}

pub struct ConfigStore;

impl CmdexConfig {
    pub fn effective_lsp_servers(&self) -> Vec<LspServerConfig> {
        if self.lsp.servers.is_empty() {
            vec![LspServerConfig::default_rust()]
        } else {
            self.lsp.servers.clone()
        }
    }
}

impl LspServerConfig {
    pub fn matches_path(&self, path: &Path) -> bool {
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            return false;
        };

        let extension = extension
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        self.extensions
            .iter()
            .any(|candidate| candidate == &extension)
    }

    fn default_rust() -> Self {
        let command = env::var_os("CMDEX_RUST_ANALYZER")
            .or_else(|| env::var_os("RUST_ANALYZER"))
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "rust-analyzer".to_string());

        Self {
            name: "rust-analyzer".to_string(),
            language_id: "rust".to_string(),
            extensions: vec!["rs".to_string()],
            command,
            args: Vec::new(),
            env: BTreeMap::new(),
        }
    }
}

impl RawLspConfig {
    fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }
}

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

        let lsp = LspConfig {
            servers: raw
                .lsp
                .servers
                .into_iter()
                .map(Self::parse_lsp_server)
                .collect::<Result<Vec<_>>>()?,
        };

        Ok(CmdexConfig { agents, lsp })
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
            lsp: RawLspConfig {
                servers: config
                    .lsp
                    .servers
                    .iter()
                    .map(|server| RawLspServerConfig {
                        name: server.name.clone(),
                        language_id: server.language_id.clone(),
                        extensions: server.extensions.clone(),
                        command: server.command.clone(),
                        args: server.args.clone(),
                        env: server.env.clone(),
                    })
                    .collect(),
            },
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

    fn parse_lsp_server(raw_server: RawLspServerConfig) -> Result<LspServerConfig> {
        let name = raw_server.name.trim().to_string();
        if name.is_empty() {
            return Err(anyhow!("LSP server name cannot be empty."));
        }

        let language_id = raw_server.language_id.trim().to_string();
        if language_id.is_empty() {
            return Err(anyhow!("LSP server '{}' must define a language_id.", name));
        }

        let command = raw_server.command.trim().to_string();
        if command.is_empty() {
            return Err(anyhow!("LSP server '{}' must define a command.", name));
        }

        let extensions = raw_server
            .extensions
            .into_iter()
            .map(|extension| {
                let normalized = extension
                    .trim()
                    .trim_start_matches('.')
                    .to_ascii_lowercase();
                if normalized.is_empty() {
                    Err(anyhow!(
                        "LSP server '{}' contains an empty file extension.",
                        name
                    ))
                } else {
                    Ok(normalized)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        if extensions.is_empty() {
            return Err(anyhow!(
                "LSP server '{}' must define at least one file extension.",
                name
            ));
        }

        Ok(LspServerConfig {
            name,
            language_id,
            extensions,
            command,
            args: raw_server.args,
            env: raw_server.env,
        })
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

    #[test]
    fn parses_lsp_servers_from_yaml() {
        let yaml = r#"
lsp:
  servers:
    - name: typescript
      language_id: typescript
      extensions: [ts, tsx]
      command: typescript-language-server
      args: [--stdio]
"#;

        let path = std::env::temp_dir().join(format!(
            "cmdex-config-lsp-{}-{}.yml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::write(&path, yaml).expect("config file");

        let config = ConfigStore::load(&path).expect("config");

        assert_eq!(config.lsp.servers.len(), 1);
        assert_eq!(config.lsp.servers[0].name, "typescript");
        assert_eq!(config.lsp.servers[0].language_id, "typescript");
        assert_eq!(config.lsp.servers[0].extensions, vec!["ts", "tsx"]);
        assert_eq!(config.lsp.servers[0].command, "typescript-language-server");
        assert_eq!(config.lsp.servers[0].args, vec!["--stdio"]);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn defaults_to_rust_lsp_when_no_servers_are_configured() {
        let config = CmdexConfig::default();
        let servers = config.effective_lsp_servers();

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].language_id, "rust");
        assert_eq!(servers[0].extensions, vec!["rs"]);
    }
}
