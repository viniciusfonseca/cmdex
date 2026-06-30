use std::{str::FromStr, sync::LazyLock};

use ratatui::style::Color as TuiColor;
use syntect::highlighting::{
    Color as SyntectColor, FontStyle as SyntectFontStyle, ScopeSelectors, StyleModifier, Theme,
    ThemeItem, ThemeSettings,
};

struct ThemeBundle {
    app: AppTheme,
    syntax: Theme,
}

pub struct ThemeRegistry;

static THEME_BUNDLE: LazyLock<ThemeBundle> = LazyLock::new(|| ThemeBundle {
    app: ThemeRegistry::hardcoded_app_theme(),
    syntax: ThemeRegistry::hardcoded_syntax_theme(),
});

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AppTheme {
    pub app_bg: TuiColor,
    pub panel_bg: TuiColor,
    pub sidebar_bg: TuiColor,
    pub input_bg: TuiColor,
    pub border: TuiColor,
    pub foreground: TuiColor,
    pub muted: TuiColor,
    pub accent: TuiColor,
    pub selection_bg: TuiColor,
    pub selection_fg: TuiColor,
    pub error: TuiColor,
    pub warning: TuiColor,
    pub success: TuiColor,
    pub info: TuiColor,
    pub red: TuiColor,
    pub green: TuiColor,
    pub yellow: TuiColor,
    pub cyan: TuiColor,
    pub magenta: TuiColor,
    pub blue: TuiColor,
    pub button_bg: TuiColor,
    pub button_fg: TuiColor,
    pub button_secondary_bg: TuiColor,
    pub button_secondary_fg: TuiColor,
    pub tab_bg: TuiColor,
    pub tab_fg: TuiColor,
    pub line_highlight: TuiColor,
    pub line_number: TuiColor,
    pub inline_code_bg: TuiColor,
    pub inline_code_fg: TuiColor,
    pub scrollbar_thumb: TuiColor,
    pub scrollbar_track: TuiColor,
}

impl ThemeRegistry {
    pub fn app() -> &'static AppTheme {
        &THEME_BUNDLE.app
    }

    pub fn syntax() -> &'static Theme {
        &THEME_BUNDLE.syntax
    }

    fn hardcoded_app_theme() -> AppTheme {
        AppTheme {
            app_bg: Self::rgb(0x26, 0x26, 0x24),
            panel_bg: Self::rgb(0x1E, 0x1E, 0x1C),
            sidebar_bg: Self::rgb(0x1E, 0x1E, 0x1C),
            input_bg: Self::rgb(0x2E, 0x2E, 0x2C),
            border: Self::rgb(0x33, 0x33, 0x31),
            foreground: Self::rgb(0xE8, 0xE4, 0xDC),
            muted: Self::rgb(0x8A, 0x8A, 0x88),
            accent: Self::rgb(0xF4, 0x84, 0x5F),
            selection_bg: Self::rgb(0x3A, 0x3A, 0x38),
            selection_fg: Self::rgb(0xE8, 0xE4, 0xDC),
            error: Self::rgb(0xF2, 0x8B, 0x82),
            warning: Self::rgb(0xE8, 0xC4, 0x7C),
            success: Self::rgb(0x7E, 0xC6, 0x99),
            info: Self::rgb(0x7C, 0xAC, 0xE8),
            red: Self::rgb(0xF2, 0x8B, 0x82),
            green: Self::rgb(0x7E, 0xC6, 0x99),
            yellow: Self::rgb(0xE8, 0xC4, 0x7C),
            cyan: Self::rgb(0x7C, 0xE8, 0xD4),
            magenta: Self::rgb(0xC4, 0x9B, 0xE8),
            blue: Self::rgb(0x7C, 0xAC, 0xE8),
            button_bg: Self::rgb(0xF4, 0x84, 0x5F),
            button_fg: Self::rgb(0xFF, 0xFF, 0xFF),
            button_secondary_bg: Self::rgb(0x3E, 0x3E, 0x3C),
            button_secondary_fg: Self::rgb(0xE8, 0xE4, 0xDC),
            tab_bg: Self::rgb(0x1E, 0x1E, 0x1C),
            tab_fg: Self::rgb(0x6A, 0x6A, 0x68),
            line_highlight: Self::rgb(0x2E, 0x2E, 0x2C),
            line_number: Self::rgb(0x4A, 0x4A, 0x48),
            inline_code_bg: Self::rgb(0x2E, 0x2E, 0x2C),
            inline_code_fg: Self::rgb(0x7C, 0xE8, 0xD4),
            scrollbar_thumb: Self::rgb(0x3E, 0x3E, 0x3C),
            scrollbar_track: Self::rgb(0x3E, 0x3E, 0x3C),
        }
    }

    fn hardcoded_syntax_theme() -> Theme {
        Theme {
            name: Some("Claude Dark (embedded)".to_string()),
            author: Some("cmdex".to_string()),
            settings: ThemeSettings {
                foreground: Some(Self::syn_rgb(0xE8, 0xE4, 0xDC)),
                background: Some(Self::syn_rgb(0x26, 0x26, 0x24)),
                caret: Some(Self::syn_rgb(0xF4, 0x84, 0x5F)),
                line_highlight: Some(Self::syn_rgb(0x2E, 0x2E, 0x2C)),
                find_highlight: Some(Self::syn_rgba(0xF4, 0x84, 0x5F, 0x44)),
                gutter: Some(Self::syn_rgb(0x26, 0x26, 0x24)),
                gutter_foreground: Some(Self::syn_rgb(0x4A, 0x4A, 0x48)),
                selection: Some(Self::syn_rgba(0x3A, 0x3A, 0x38, 0x88)),
                inactive_selection: Some(Self::syn_rgba(0x3A, 0x3A, 0x38, 0x44)),
                guide: Some(Self::syn_rgb(0x33, 0x33, 0x31)),
                active_guide: Some(Self::syn_rgb(0x3A, 0x3A, 0x38)),
                shadow: Some(Self::syn_rgba(0x00, 0x00, 0x00, 0x33)),
                ..ThemeSettings::default()
            },
            scopes: vec![
                Self::theme_item(
                    "comment, punctuation.definition.comment",
                    Self::syn_rgb(0x5A, 0x5A, 0x58),
                    None,
                    Some("italic"),
                ),
                Self::theme_item(
                    "string, string.quoted, string.template",
                    Self::syn_rgb(0x7E, 0xC6, 0x99),
                    None,
                    None,
                ),
                Self::theme_item("string.regexp", Self::syn_rgb(0x7C, 0xE8, 0xD4), None, None),
                Self::theme_item(
                    "constant.numeric, constant.language, constant.character",
                    Self::syn_rgb(0xF4, 0xA5, 0x8A),
                    None,
                    None,
                ),
                Self::theme_item(
                    "constant.other",
                    Self::syn_rgb(0xE8, 0xC4, 0x7C),
                    None,
                    None,
                ),
                Self::theme_item(
                    "keyword, keyword.control, keyword.operator.new, keyword.operator.expression, keyword.operator.logical, storage.type, storage.modifier",
                    Self::syn_rgb(0xF4, 0x84, 0x5F),
                    None,
                    None,
                ),
                Self::theme_item(
                    "keyword.operator, punctuation.accessor",
                    Self::syn_rgb(0xC0, 0xBC, 0xAF),
                    None,
                    None,
                ),
                Self::theme_item(
                    "entity.name.function, meta.function-call, support.function",
                    Self::syn_rgb(0x7C, 0xAC, 0xE8),
                    None,
                    None,
                ),
                Self::theme_item(
                    "entity.name.type, entity.name.class, support.type, support.class, entity.other.inherited-class",
                    Self::syn_rgb(0xE8, 0xC4, 0x7C),
                    None,
                    None,
                ),
                Self::theme_item(
                    "entity.name.type.interface, entity.name.type.type-parameter",
                    Self::syn_rgb(0x7C, 0xE8, 0xD4),
                    None,
                    None,
                ),
                Self::theme_item(
                    "variable, variable.other",
                    Self::syn_rgb(0xE8, 0xE4, 0xDC),
                    None,
                    None,
                ),
                Self::theme_item(
                    "variable.parameter, variable.other.readwrite",
                    Self::syn_rgb(0xD4, 0xCF, 0xBF),
                    None,
                    None,
                ),
                Self::theme_item(
                    "variable.language",
                    Self::syn_rgb(0xF4, 0x84, 0x5F),
                    None,
                    Some("italic"),
                ),
                Self::theme_item(
                    "variable.other.property, variable.other.object.property, support.variable.property, meta.object-literal.key",
                    Self::syn_rgb(0xC4, 0x9B, 0xE8),
                    None,
                    None,
                ),
                Self::theme_item(
                    "entity.name.tag, support.class.component",
                    Self::syn_rgb(0xF4, 0x84, 0x5F),
                    None,
                    None,
                ),
                Self::theme_item(
                    "entity.other.attribute-name",
                    Self::syn_rgb(0xE8, 0xC4, 0x7C),
                    None,
                    Some("italic"),
                ),
                Self::theme_item(
                    "punctuation.definition.tag, punctuation.definition.block, punctuation.definition.parameters, punctuation.section, meta.brace",
                    Self::syn_rgb(0x8A, 0x8A, 0x88),
                    None,
                    None,
                ),
                Self::theme_item(
                    "punctuation.separator, punctuation.terminator",
                    Self::syn_rgb(0x6A, 0x6A, 0x68),
                    None,
                    None,
                ),
                Self::theme_item(
                    "meta.decorator, punctuation.decorator",
                    Self::syn_rgb(0xE8, 0xC4, 0x7C),
                    None,
                    Some("italic"),
                ),
                Self::theme_item(
                    "markup.heading, entity.name.section",
                    Self::syn_rgb(0xF4, 0x84, 0x5F),
                    None,
                    Some("bold"),
                ),
                Self::theme_item(
                    "markup.bold",
                    Self::syn_rgb(0xE8, 0xE4, 0xDC),
                    None,
                    Some("bold"),
                ),
                Self::theme_item(
                    "markup.italic",
                    Self::syn_rgb(0xC4, 0x9B, 0xE8),
                    None,
                    Some("italic"),
                ),
                Self::theme_item(
                    "markup.inline.raw, markup.fenced_code",
                    Self::syn_rgb(0x7C, 0xE8, 0xD4),
                    None,
                    None,
                ),
                Self::theme_item(
                    "markup.underline.link",
                    Self::syn_rgb(0x7C, 0xAC, 0xE8),
                    None,
                    None,
                ),
                Self::theme_item("markup.list", Self::syn_rgb(0xF4, 0xA5, 0x8A), None, None),
                Self::theme_item(
                    "markup.quote",
                    Self::syn_rgb(0x6A, 0x6A, 0x68),
                    None,
                    Some("italic"),
                ),
                Self::theme_item(
                    "support.type.property-name.css",
                    Self::syn_rgb(0x7C, 0xAC, 0xE8),
                    None,
                    None,
                ),
                Self::theme_item(
                    "support.constant.property-value.css, constant.other.color.rgb-value.hex.css",
                    Self::syn_rgb(0xF4, 0xA5, 0x8A),
                    None,
                    None,
                ),
                Self::theme_item(
                    "entity.other.attribute-name.class.css, entity.other.attribute-name.id.css",
                    Self::syn_rgb(0xE8, 0xC4, 0x7C),
                    None,
                    None,
                ),
                Self::theme_item(
                    "keyword.other.unit.css",
                    Self::syn_rgb(0xF4, 0x84, 0x5F),
                    None,
                    None,
                ),
                Self::theme_item(
                    "support.type.property-name.json",
                    Self::syn_rgb(0xC4, 0x9B, 0xE8),
                    None,
                    None,
                ),
                Self::theme_item(
                    "entity.name.tag.yaml",
                    Self::syn_rgb(0x7C, 0xAC, 0xE8),
                    None,
                    None,
                ),
                Self::theme_item(
                    "source.shell variable.other",
                    Self::syn_rgb(0x7C, 0xE8, 0xD4),
                    None,
                    None,
                ),
                Self::theme_item(
                    "keyword.control.import, keyword.control.export, keyword.control.from",
                    Self::syn_rgb(0xF4, 0x84, 0x5F),
                    None,
                    None,
                ),
                Self::theme_item(
                    "variable.other.readwrite.alias",
                    Self::syn_rgb(0xE8, 0xC4, 0x7C),
                    None,
                    None,
                ),
                Self::theme_item(
                    "string.template meta.embedded, meta.template.expression",
                    Self::syn_rgb(0xE8, 0xE4, 0xDC),
                    None,
                    None,
                ),
                Self::theme_item(
                    "punctuation.definition.template-expression",
                    Self::syn_rgb(0xF4, 0x84, 0x5F),
                    None,
                    None,
                ),
            ],
        }
    }

    fn theme_item(
        scope: &str,
        foreground: SyntectColor,
        background: Option<SyntectColor>,
        font_style: Option<&str>,
    ) -> ThemeItem {
        ThemeItem {
            scope: ScopeSelectors::from_str(scope).expect("embedded theme scope should be valid"),
            style: StyleModifier {
                foreground: Some(foreground),
                background,
                font_style: font_style.map(Self::parse_font_style),
            },
        }
    }

    fn parse_font_style(value: &str) -> SyntectFontStyle {
        let mut font_style = SyntectFontStyle::empty();
        for token in value.split_whitespace() {
            match token {
                "bold" => font_style.insert(SyntectFontStyle::BOLD),
                "italic" => font_style.insert(SyntectFontStyle::ITALIC),
                "underline" => font_style.insert(SyntectFontStyle::UNDERLINE),
                _ => {}
            }
        }
        font_style
    }

    const fn rgb(r: u8, g: u8, b: u8) -> TuiColor {
        TuiColor::Rgb(r, g, b)
    }

    const fn syn_rgb(r: u8, g: u8, b: u8) -> SyntectColor {
        SyntectColor { r, g, b, a: 0xFF }
    }

    const fn syn_rgba(r: u8, g: u8, b: u8, a: u8) -> SyntectColor {
        SyntectColor { r, g, b, a }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_expected_app_theme_colors() {
        let theme = ThemeRegistry::app();

        assert_eq!(theme.app_bg, ThemeRegistry::rgb(0x26, 0x26, 0x24));
        assert_eq!(theme.accent, ThemeRegistry::rgb(0xF4, 0x84, 0x5F));
        assert_eq!(theme.yellow, ThemeRegistry::rgb(0xE8, 0xC4, 0x7C));
    }

    #[test]
    fn embeds_syntax_theme_rules() {
        let theme = ThemeRegistry::syntax();

        assert_eq!(theme.name.as_deref(), Some("Claude Dark (embedded)"));
        assert!(theme.scopes.len() >= 30);
        assert_eq!(
            theme.settings.background,
            Some(ThemeRegistry::syn_rgb(0x26, 0x26, 0x24))
        );
    }
}
