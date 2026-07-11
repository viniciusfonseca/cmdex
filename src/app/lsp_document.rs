use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use url::Url;

use super::LspServerConfig;

fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| anyhow!("failed to convert {} into a file URI", path.display()))
}

#[derive(Debug, Clone)]
struct TrackedDocument {
    version: i32,
    source: String,
}

#[derive(Debug, Default)]
pub(super) struct DocumentSync {
    documents: HashMap<PathBuf, TrackedDocument>,
}

impl DocumentSync {
    pub(super) fn notification_for(
        &mut self,
        server: &LspServerConfig,
        path: &Path,
        source: &str,
    ) -> Result<Option<(&'static str, Value)>> {
        let uri = file_uri(path)?;

        if let Some((version, changed)) = self
            .documents
            .get(path)
            .map(|document| (document.version + 1, document.source.as_str() != source))
        {
            if !changed {
                return Ok(None);
            }

            if let Some(document) = self.documents.get_mut(path) {
                document.version = version;
                document.source = source.to_string();
            }
            return Ok(Some((
                "textDocument/didChange",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "version": version,
                    },
                    "contentChanges": [{
                        "text": source,
                    }]
                }),
            )));
        }

        self.documents.insert(
            path.to_path_buf(),
            TrackedDocument {
                version: 1,
                source: source.to_string(),
            },
        );
        Ok(Some((
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": server.language_id,
                    "version": 1,
                    "text": source,
                }
            }),
        )))
    }

    pub(super) fn clear(&mut self) {
        self.documents.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> LspServerConfig {
        LspServerConfig {
            name: "rust-analyzer".to_string(),
            command: "rust-analyzer".to_string(),
            args: Vec::new(),
            language_id: "rust".to_string(),
            extensions: vec!["rs".to_string()],
            env: Default::default(),
        }
    }

    #[test]
    fn opens_document_once_and_only_emits_changes_when_source_changes() {
        let mut sync = DocumentSync::default();
        let path = PathBuf::from("/tmp/main.rs");

        let open = sync
            .notification_for(&server(), &path, "fn main() {}")
            .unwrap()
            .unwrap();
        assert_eq!(open.0, "textDocument/didOpen");

        assert!(
            sync.notification_for(&server(), &path, "fn main() {}")
                .unwrap()
                .is_none()
        );

        let change = sync
            .notification_for(&server(), &path, "fn main() { println!(); }")
            .unwrap()
            .unwrap();
        assert_eq!(change.0, "textDocument/didChange");
        assert_eq!(change.1["textDocument"]["version"], 2);
    }
}
