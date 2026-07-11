use super::*;

pub(super) fn app_with_agent(workspace: impl Into<PathBuf>) -> App {
    App::new(
        PathBuf::new(),
        CmdexConfig {
            agents: vec![agent_definition(workspace)],
            ..CmdexConfig::default()
        },
    )
}

pub(super) fn agent_definition(workspace: impl Into<PathBuf>) -> AgentDefinition {
    AgentDefinition {
        name: "Test".to_string(),
        workspace: workspace.into(),
    }
}

pub(super) fn model(id: &str, display_name: &str, is_default: bool, efforts: &[&str]) -> ModelInfo {
    ModelInfo {
        id: id.to_string(),
        model: id.to_string(),
        display_name: display_name.to_string(),
        is_default,
        supported_reasoning_efforts: efforts
            .iter()
            .map(|effort| ModelReasoningEffort {
                reasoning_effort: (*effort).to_string(),
                description: None,
            })
            .collect(),
        default_reasoning_effort: efforts.first().map(|effort| (*effort).to_string()),
    }
}
