use std::{path::Path, sync::LazyLock};

use syntect::parsing::SyntaxSet;

const EXTERNAL_SYNTAXES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/syntaxes");

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxRegistry::load);

pub struct SyntaxRegistry;

impl SyntaxRegistry {
    pub fn set() -> &'static SyntaxSet {
        &SYNTAX_SET
    }

    fn load() -> SyntaxSet {
        let external_syntaxes_dir = Path::new(EXTERNAL_SYNTAXES_DIR);
        if !external_syntaxes_dir.is_dir() {
            return SyntaxSet::load_defaults_newlines();
        }

        let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
        if let Err(error) = builder.add_from_folder(external_syntaxes_dir, true) {
            eprintln!(
                "failed to load external syntaxes from {}: {error}",
                external_syntaxes_dir.display()
            );
            return SyntaxSet::load_defaults_newlines();
        }

        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use super::SyntaxRegistry;

    #[test]
    fn loads_kr_syntax_from_external_assets() {
        let syntax = SyntaxRegistry::set()
            .find_syntax_by_extension("kr")
            .expect("expected KR syntax to be available");

        assert_eq!(syntax.name, "KR");
    }
}
