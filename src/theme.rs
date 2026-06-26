use std::{collections::HashMap, env, fs, path::PathBuf, str::FromStr, sync::LazyLock};

use ratatui::style::Color as TuiColor;
use serde::Deserialize;
use syntect::highlighting::{
    Color as SyntectColor, FontStyle as SyntectFontStyle, ScopeSelectors, StyleModifier, Theme,
    ThemeItem, ThemeSet,
};

const THEME_RELATIVE_PATH: &str =
    ".vscode/extensions/duca.claude-anthropic-theme-1.0.0/themes/claude-dark-color-theme.json";

struct ThemeBundle {
    app: AppTheme,
    syntax: Theme,
}

static THEME_BUNDLE: LazyLock<ThemeBundle> = LazyLock::new(load_theme_bundle);

pub fn app_theme() -> &'static AppTheme {
    &THEME_BUNDLE.app
}

pub fn syntax_theme() -> &'static Theme {
    &THEME_BUNDLE.syntax
}

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

impl AppTheme {
    fn fallback() -> Self {
        Self {
            app_bg: rgb(0x26, 0x26, 0x24),
            panel_bg: rgb(0x1E, 0x1E, 0x1C),
            sidebar_bg: rgb(0x1E, 0x1E, 0x1C),
            input_bg: rgb(0x2E, 0x2E, 0x2C),
            border: rgb(0x33, 0x33, 0x31),
            foreground: rgb(0xE8, 0xE4, 0xDC),
            muted: rgb(0x8A, 0x8A, 0x88),
            accent: rgb(0xF4, 0x84, 0x5F),
            selection_bg: rgb(0x3A, 0x3A, 0x38),
            selection_fg: rgb(0xE8, 0xE4, 0xDC),
            error: rgb(0xF2, 0x8B, 0x82),
            warning: rgb(0xE8, 0xC4, 0x7C),
            success: rgb(0x7E, 0xC6, 0x99),
            info: rgb(0x7C, 0xAC, 0xE8),
            red: rgb(0xF2, 0x8B, 0x82),
            green: rgb(0x7E, 0xC6, 0x99),
            yellow: rgb(0xE8, 0xC4, 0x7C),
            cyan: rgb(0x7C, 0xE8, 0xD4),
            magenta: rgb(0xC4, 0x9B, 0xE8),
            blue: rgb(0x7C, 0xAC, 0xE8),
            button_bg: rgb(0xF4, 0x84, 0x5F),
            button_fg: rgb(0xFF, 0xFF, 0xFF),
            button_secondary_bg: rgb(0x3E, 0x3E, 0x3C),
            button_secondary_fg: rgb(0xE8, 0xE4, 0xDC),
            tab_bg: rgb(0x1E, 0x1E, 0x1C),
            tab_fg: rgb(0x6A, 0x6A, 0x68),
            line_highlight: rgb(0x2E, 0x2E, 0x2C),
            line_number: rgb(0x4A, 0x4A, 0x48),
            inline_code_bg: rgb(0x2E, 0x2E, 0x2C),
            inline_code_fg: rgb(0x7C, 0xE8, 0xD4),
            scrollbar_thumb: rgb(0x3E, 0x3E, 0x3C),
            scrollbar_track: rgb(0x26, 0x26, 0x24),
        }
    }
}

#[derive(Debug, Deserialize)]
struct VscodeThemeFile {
    name: Option<String>,
    #[serde(default)]
    colors: HashMap<String, String>,
    #[serde(default, rename = "tokenColors")]
    token_colors: Vec<VscodeTokenColorRule>,
}

#[derive(Debug, Deserialize)]
struct VscodeTokenColorRule {
    #[serde(default)]
    scope: Option<VscodeScopeField>,
    #[serde(default)]
    settings: VscodeTokenSettings,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum VscodeScopeField {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Default, Deserialize)]
struct VscodeTokenSettings {
    foreground: Option<String>,
    background: Option<String>,
    #[serde(rename = "fontStyle")]
    font_style: Option<String>,
}

fn load_theme_bundle() -> ThemeBundle {
    if let Some(theme_file) = load_vscode_theme_file() {
        let app = build_app_theme(&theme_file);
        let syntax = build_syntect_theme(&theme_file).unwrap_or_else(fallback_syntax_theme);
        ThemeBundle { app, syntax }
    } else {
        ThemeBundle {
            app: AppTheme::fallback(),
            syntax: fallback_syntax_theme(),
        }
    }
}

fn load_vscode_theme_file() -> Option<VscodeThemeFile> {
    let home = env::var("HOME").ok()?;
    let path = PathBuf::from(home).join(THEME_RELATIVE_PATH);
    let source = fs::read_to_string(path).ok()?;
    let stripped = strip_json_comments(&source);
    serde_json::from_str(&stripped).ok()
}

fn build_app_theme(theme: &VscodeThemeFile) -> AppTheme {
    let fallback = AppTheme::fallback();
    let color = |key: &str| theme_color_tui(&theme.colors, key);
    let pick = |key: &str, default: TuiColor| color(key).unwrap_or(default);

    AppTheme {
        app_bg: pick("editor.background", fallback.app_bg),
        panel_bg: pick("panel.background", fallback.panel_bg),
        sidebar_bg: pick("sideBar.background", fallback.sidebar_bg),
        input_bg: pick("input.background", fallback.input_bg),
        border: color("panel.border")
            .or_else(|| color("sideBar.border"))
            .unwrap_or(fallback.border),
        foreground: pick("editor.foreground", fallback.foreground),
        muted: color("statusBar.foreground")
            .or_else(|| color("titleBar.inactiveForeground"))
            .unwrap_or(fallback.muted),
        accent: color("button.background")
            .or_else(|| color("editorCursor.foreground"))
            .unwrap_or(fallback.accent),
        selection_bg: color("list.activeSelectionBackground")
            .or_else(|| color("editor.selectionBackground"))
            .unwrap_or(fallback.selection_bg),
        selection_fg: color("list.activeSelectionForeground").unwrap_or(fallback.selection_fg),
        error: color("editorError.foreground")
            .or_else(|| color("terminal.ansiRed"))
            .unwrap_or(fallback.error),
        warning: color("editorWarning.foreground")
            .or_else(|| color("terminal.ansiYellow"))
            .unwrap_or(fallback.warning),
        success: color("gitDecoration.addedResourceForeground")
            .or_else(|| color("terminal.ansiGreen"))
            .unwrap_or(fallback.success),
        info: color("editorInfo.foreground")
            .or_else(|| color("terminal.ansiBlue"))
            .unwrap_or(fallback.info),
        red: pick("terminal.ansiRed", fallback.red),
        green: pick("terminal.ansiGreen", fallback.green),
        yellow: pick("terminal.ansiYellow", fallback.yellow),
        cyan: pick("terminal.ansiCyan", fallback.cyan),
        magenta: pick("terminal.ansiMagenta", fallback.magenta),
        blue: pick("terminal.ansiBlue", fallback.blue),
        button_bg: pick("button.background", fallback.button_bg),
        button_fg: pick("button.foreground", fallback.button_fg),
        button_secondary_bg: pick("button.secondaryBackground", fallback.button_secondary_bg),
        button_secondary_fg: pick("button.secondaryForeground", fallback.button_secondary_fg),
        tab_bg: color("editorGroupHeader.tabsBackground")
            .or_else(|| color("tab.inactiveBackground"))
            .unwrap_or(fallback.tab_bg),
        tab_fg: color("tab.inactiveForeground").unwrap_or(fallback.tab_fg),
        line_highlight: pick("editor.lineHighlightBackground", fallback.line_highlight),
        line_number: pick("editorLineNumber.foreground", fallback.line_number),
        inline_code_bg: color("editorWidget.background").unwrap_or(fallback.inline_code_bg),
        inline_code_fg: pick("terminal.ansiCyan", fallback.inline_code_fg),
        scrollbar_thumb: color("scrollbarSlider.activeBackground")
            .or_else(|| color("scrollbarSlider.background"))
            .unwrap_or(fallback.scrollbar_thumb),
        scrollbar_track: color("scrollbarSlider.background").unwrap_or(fallback.scrollbar_track),
    }
}

fn build_syntect_theme(theme: &VscodeThemeFile) -> Option<Theme> {
    let scopes = theme
        .token_colors
        .iter()
        .filter_map(|rule| {
            let scope = scope_selector_text(rule.scope.as_ref())?;
            let scope = ScopeSelectors::from_str(&scope).ok()?;
            let style = StyleModifier {
                foreground: rule
                    .settings
                    .foreground
                    .as_deref()
                    .and_then(parse_hex_syntect_color),
                background: rule
                    .settings
                    .background
                    .as_deref()
                    .and_then(parse_hex_syntect_color),
                font_style: rule.settings.font_style.as_deref().map(parse_font_style),
            };

            if style.foreground.is_none()
                && style.background.is_none()
                && style.font_style.is_none()
            {
                return None;
            }

            Some(ThemeItem { scope, style })
        })
        .collect::<Vec<_>>();

    let settings = syntect::highlighting::ThemeSettings {
        foreground: theme_color_syntect(&theme.colors, "editor.foreground"),
        background: theme_color_syntect(&theme.colors, "editor.background"),
        caret: theme_color_syntect(&theme.colors, "editorCursor.foreground"),
        line_highlight: theme_color_syntect(&theme.colors, "editor.lineHighlightBackground"),
        find_highlight: theme_color_syntect(&theme.colors, "editor.findMatchBackground"),
        gutter: theme_color_syntect(&theme.colors, "editor.background"),
        gutter_foreground: theme_color_syntect(&theme.colors, "editorLineNumber.foreground"),
        selection: theme_color_syntect(&theme.colors, "editor.selectionBackground"),
        inactive_selection: theme_color_syntect(
            &theme.colors,
            "editor.inactiveSelectionBackground",
        ),
        guide: theme_color_syntect(&theme.colors, "editorIndentGuide.background1"),
        active_guide: theme_color_syntect(&theme.colors, "editorIndentGuide.activeBackground1"),
        shadow: theme_color_syntect(&theme.colors, "scrollbar.shadow"),
        ..Default::default()
    };

    Some(Theme {
        name: theme.name.clone(),
        author: None,
        settings,
        scopes,
    })
}

fn fallback_syntax_theme() -> Theme {
    let themes = ThemeSet::load_defaults();
    themes
        .themes
        .get("base16-ocean.dark")
        .cloned()
        .unwrap_or_default()
}

fn scope_selector_text(scope: Option<&VscodeScopeField>) -> Option<String> {
    match scope? {
        VscodeScopeField::Single(value) => Some(value.clone()),
        VscodeScopeField::Multiple(values) if !values.is_empty() => Some(values.join(", ")),
        VscodeScopeField::Multiple(_) => None,
    }
}

fn theme_color_tui(colors: &HashMap<String, String>, key: &str) -> Option<TuiColor> {
    colors.get(key).and_then(|value| parse_hex_tui_color(value))
}

fn theme_color_syntect(colors: &HashMap<String, String>, key: &str) -> Option<SyntectColor> {
    colors
        .get(key)
        .and_then(|value| parse_hex_syntect_color(value))
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

fn parse_hex_tui_color(value: &str) -> Option<TuiColor> {
    let rgba = parse_hex_rgba(value)?;
    Some(TuiColor::Rgb(rgba[0], rgba[1], rgba[2]))
}

fn parse_hex_syntect_color(value: &str) -> Option<SyntectColor> {
    let rgba = parse_hex_rgba(value)?;
    Some(SyntectColor {
        r: rgba[0],
        g: rgba[1],
        b: rgba[2],
        a: rgba[3],
    })
}

fn parse_hex_rgba(value: &str) -> Option<[u8; 4]> {
    let hex = value.strip_prefix('#')?;
    match hex.len() {
        6 => Some([
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            0xFF,
        ]),
        8 => Some([
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            u8::from_str_radix(&hex[6..8], 16).ok()?,
        ]),
        _ => None,
    }
}

fn strip_json_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let chars = source.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    while index < chars.len() {
        let current = chars[index];

        if in_string {
            output.push(current);
            if escaped {
                escaped = false;
            } else if current == '\\' {
                escaped = true;
            } else if current == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        if current == '"' {
            in_string = true;
            output.push(current);
            index += 1;
            continue;
        }

        if current == '/' && index + 1 < chars.len() {
            match chars[index + 1] {
                '/' => {
                    index += 2;
                    while index < chars.len() && chars[index] != '\n' {
                        index += 1;
                    }
                    continue;
                }
                '*' => {
                    index += 2;
                    while index + 1 < chars.len()
                        && !(chars[index] == '*' && chars[index + 1] == '/')
                    {
                        index += 1;
                    }
                    index = (index + 2).min(chars.len());
                    continue;
                }
                _ => {}
            }
        }

        output.push(current);
        index += 1;
    }

    output
}

const fn rgb(r: u8, g: u8, b: u8) -> TuiColor {
    TuiColor::Rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_json_comments_without_touching_strings() {
        let source = r#"
        {
          "url": "https://example.com//path",
          // comment
          "value": "/* keep */"
        }
        "#;

        let stripped = strip_json_comments(source);

        assert!(stripped.contains("https://example.com//path"));
        assert!(stripped.contains("/* keep */"));
        assert!(!stripped.contains("// comment"));
    }

    #[test]
    fn parses_hex_rgba_values() {
        assert_eq!(parse_hex_rgba("#F4845F"), Some([0xF4, 0x84, 0x5F, 0xFF]));
        assert_eq!(parse_hex_rgba("#3A3A3888"), Some([0x3A, 0x3A, 0x38, 0x88]));
    }
}
