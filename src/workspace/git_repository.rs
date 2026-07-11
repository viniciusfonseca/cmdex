use std::{path::Path, process::Command};

use anyhow::{Context, Result};

use super::{
    DiffSection, GitDiffLoadResult, GitMutation, GitRemoteAction, PREVIEW_LIMIT,
    diff::{DiffSections, GitDiffSupport},
    render::WorkspaceRenderer,
};

/// Synchronous Git boundary. Callers must run these methods outside the UI thread.
pub(crate) struct GitRepository<'a> {
    root: &'a Path,
}

impl<'a> GitRepository<'a> {
    pub(crate) fn new(root: &'a Path) -> Self {
        Self { root }
    }

    pub(crate) fn load_snapshot(
        &self,
        requested_section: DiffSection,
        selected_path: Option<&Path>,
    ) -> Result<GitDiffLoadResult> {
        let sections = self.build_diff_entries()?;
        let active_section = match requested_section {
            DiffSection::Changes if sections.changes.is_empty() && !sections.staged.is_empty() => {
                DiffSection::Staged
            }
            DiffSection::Staged if sections.staged.is_empty() && !sections.changes.is_empty() => {
                DiffSection::Changes
            }
            section => section,
        };
        let entries = match active_section {
            DiffSection::Changes => &sections.changes,
            DiffSection::Staged => &sections.staged,
        };
        let entry = selected_path
            .and_then(|path| entries.iter().find(|entry| entry.path == path))
            .or_else(|| entries.first());
        let selected_path = entry.map(|entry| entry.path.clone());
        let (preview_title, preview) = if let Some(entry) = entry {
            (
                entry.path.display().to_string(),
                GitDiffSupport::read_diff_preview(self.root, entry, active_section)?,
            )
        } else {
            (
                "Git Diff".to_string(),
                WorkspaceRenderer::plain_preview_lines("No changes found."),
            )
        };

        Ok(GitDiffLoadResult {
            changes: sections.changes,
            staged: sections.staged,
            active_section,
            selected_path,
            preview_title,
            preview,
        })
    }

    pub(crate) fn run_mutation(&self, mutation: &GitMutation) -> Result<String> {
        match mutation {
            GitMutation::Commit(message) => self.run_git_command(&["commit", "-m", message]),
            GitMutation::Stage(path) => self.run_git_command(&["add", "--", path]),
            GitMutation::Unstage(path) => {
                self.run_git_command(&["restore", "--staged", "--", path])
            }
            GitMutation::Discard { path, untracked } => {
                if *untracked {
                    self.run_git_command(&["clean", "-f", "--", path])
                } else {
                    self.run_git_command(&["restore", "--", path])
                }
            }
        }
    }

    pub(crate) fn run_remote_action(&self, action: GitRemoteAction) -> Result<String> {
        match action {
            GitRemoteAction::Push => self.run_git_command(&["push"]),
            GitRemoteAction::Pull => self.run_git_command(&["pull", "--ff-only"]),
        }
    }

    pub(crate) fn relative_path(&self, path: &Path) -> Result<String> {
        path.strip_prefix(self.root)
            .unwrap_or(path)
            .to_str()
            .map(ToString::to_string)
            .ok_or_else(|| {
                anyhow::anyhow!("Path contains unsupported characters: {}", path.display())
            })
    }

    pub(crate) fn output(&self, args: &[&str]) -> Result<String> {
        let output = self.run(args)?;
        if !output.status.success() {
            return Ok(String::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if stdout.len() > PREVIEW_LIMIT {
            Ok(format!("{}\n\n[truncated]", &stdout[..PREVIEW_LIMIT]))
        } else {
            Ok(stdout)
        }
    }

    pub(crate) fn output_bytes(&self, args: &[&str]) -> Result<Vec<u8>> {
        let output = self.run(args)?;
        if !output.status.success() {
            return Ok(Vec::new());
        }
        Ok(output.stdout)
    }

    fn build_diff_entries(&self) -> Result<DiffSections> {
        let output = self
            .run(&["status", "--short", "--untracked-files=all"])
            .with_context(|| format!("failed to run git status in {}", self.root.display()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(stderr.trim().to_string()));
        }
        Ok(GitDiffSupport::parse_status_sections(
            self.root,
            &String::from_utf8_lossy(&output.stdout),
        ))
    }

    fn run_git_command(&self, args: &[&str]) -> Result<String> {
        let output = self.run_with_prompt_disabled(args)?;
        let stdout =
            WorkspaceRenderer::normalize_newlines(&String::from_utf8_lossy(&output.stdout));
        let stderr =
            WorkspaceRenderer::normalize_newlines(&String::from_utf8_lossy(&output.stderr));
        let summary = Self::summarize_output(&stdout, &stderr);
        if !output.status.success() {
            return Err(anyhow::anyhow!(if summary.is_empty() {
                format!("git {:?} failed", args)
            } else {
                summary
            }));
        }
        Ok(if summary.is_empty() {
            format!("git {} finished successfully", args.join(" "))
        } else {
            summary
        })
    }

    fn run(&self, args: &[&str]) -> Result<std::process::Output> {
        Command::new("git")
            .arg("-C")
            .arg(self.root)
            .args(args)
            .output()
            .with_context(|| format!("failed to run git {:?} in {}", args, self.root.display()))
    }

    fn run_with_prompt_disabled(&self, args: &[&str]) -> Result<std::process::Output> {
        Command::new("git")
            .arg("-C")
            .arg(self.root)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .with_context(|| format!("failed to run git {:?} in {}", args, self.root.display()))
    }

    fn summarize_output(stdout: &str, stderr: &str) -> String {
        let combined = [stdout.trim(), stderr.trim()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if combined.is_empty() {
            return String::new();
        }

        let mut summary = combined.lines().take(3).collect::<Vec<_>>().join(" ");
        if summary.chars().count() > 240 {
            summary = summary.chars().take(237).collect::<String>();
            summary.push_str("...");
        }
        summary
    }
}
