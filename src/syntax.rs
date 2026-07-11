use std::{path::Path, sync::LazyLock};

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::{easy::HighlightLines, parsing::SyntaxReference, parsing::SyntaxSet};

use crate::theme::ThemeRegistry;

const EXTERNAL_SYNTAXES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/syntaxes");

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxRegistry::load);

pub struct SyntaxRegistry;

impl SyntaxRegistry {
    pub fn set() -> &'static SyntaxSet {
        &SYNTAX_SET
    }

    pub fn syntax_for_path(path: &Path, source: &str) -> Option<&'static SyntaxReference> {
        path.extension()
            .and_then(|extension| extension.to_str())
            .and_then(|extension| Self::set().find_syntax_by_extension(extension))
            .or_else(|| {
                source
                    .lines()
                    .next()
                    .and_then(|line| Self::set().find_syntax_by_first_line(line))
            })
    }

    pub fn syntax_for_path_or_language(
        path: &Path,
        language: Option<&str>,
        source: &str,
    ) -> Option<&'static SyntaxReference> {
        Self::syntax_for_path(path, source).or_else(|| {
            let language = language.unwrap_or_default().trim();
            (!language.is_empty()).then(|| {
                Self::set()
                    .find_syntax_by_token(language)
                    .or_else(|| Self::set().find_syntax_by_name(language))
                    .or_else(|| Self::set().find_syntax_by_extension(language))
            })?
        })
    }

    pub fn highlight_source(
        source: &str,
        syntax: Option<&'static SyntaxReference>,
    ) -> Vec<Line<'static>> {
        let lines = if source.is_empty() {
            vec![String::new()]
        } else {
            source.split('\n').map(ToString::to_string).collect()
        };
        let Some(syntax) = syntax else {
            return lines.into_iter().map(Line::from).collect();
        };

        let mut highlighter = HighlightLines::new(syntax, ThemeRegistry::syntax());
        lines
            .into_iter()
            .map(|line| {
                if line.is_empty() {
                    return Line::default();
                }
                match highlighter.highlight_line(&line, Self::set()) {
                    Ok(ranges) => Line::from(
                        ranges
                            .into_iter()
                            .map(|(style, text)| {
                                Span::styled(text.to_string(), Self::to_ratatui_style(style))
                            })
                            .collect::<Vec<_>>(),
                    ),
                    Err(_) => Line::from(line),
                }
            })
            .collect()
    }

    pub fn highlight_path(path: &Path, source: &str) -> Vec<Line<'static>> {
        Self::highlight_source(source, Self::syntax_for_path(path, source))
    }

    pub fn highlight_path_or_language(
        path: &Path,
        language: Option<&str>,
        source: &str,
    ) -> Vec<Line<'static>> {
        Self::highlight_source(
            source,
            Self::syntax_for_path_or_language(path, language, source),
        )
    }

    fn to_ratatui_style(style: syntect::highlighting::Style) -> Style {
        let mut modifiers = Modifier::empty();
        if style
            .font_style
            .contains(syntect::highlighting::FontStyle::BOLD)
        {
            modifiers |= Modifier::BOLD;
        }
        if style
            .font_style
            .contains(syntect::highlighting::FontStyle::ITALIC)
        {
            modifiers |= Modifier::ITALIC;
        }
        if style
            .font_style
            .contains(syntect::highlighting::FontStyle::UNDERLINE)
        {
            modifiers |= Modifier::UNDERLINED;
        }

        Style::default()
            .fg(Color::Rgb(
                style.foreground.r,
                style.foreground.g,
                style.foreground.b,
            ))
            .bg(Color::Rgb(
                style.background.r,
                style.background.g,
                style.background.b,
            ))
            .add_modifier(modifiers)
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
