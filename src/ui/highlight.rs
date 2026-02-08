use ratatui::style::{Color, Modifier, Style};
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Holds syntect resources for syntax highlighting.
/// Created once and reused for the lifetime of the application.
pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme: syntect::highlighting::Theme,
}

impl Highlighter {
    /// Create a new highlighter with default syntax definitions and theme.
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        // Use a dark theme suitable for terminals
        let theme = theme_set
            .themes
            .get("base16-ocean.dark")
            .or_else(|| theme_set.themes.get("base16-eighties.dark"))
            .cloned()
            .unwrap_or_else(|| theme_set.themes.values().next().unwrap().clone());

        Self { syntax_set, theme }
    }

    /// Highlight source code and return a Vec of (style, ranges) per line.
    /// Each line is a Vec of (ratatui::Style, String) spans.
    pub fn highlight_lines(&self, source: &str, language: &str) -> Vec<Vec<(Style, String)>> {
        use syntect::easy::HighlightLines;

        let syntax = self
            .syntax_set
            .find_syntax_by_token(language)
            .or_else(|| self.syntax_set.find_syntax_by_extension(language))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let mut h = HighlightLines::new(syntax, &self.theme);
        let mut result = Vec::new();

        for line in source.lines() {
            let line_with_newline = format!("{}\n", line);
            let ranges = h
                .highlight_line(&line_with_newline, &self.syntax_set)
                .unwrap_or_default();

            let spans: Vec<(Style, String)> = ranges
                .into_iter()
                .map(|(style, text)| {
                    let ratatui_style = syntect_style_to_ratatui(style);
                    // Strip trailing newline we added
                    let text = text.trim_end_matches('\n').to_string();
                    (ratatui_style, text)
                })
                .filter(|(_, text)| !text.is_empty())
                .collect();

            result.push(spans);
        }

        // If source is empty, return one empty line
        if result.is_empty() {
            result.push(vec![]);
        }

        result
    }
}

/// Convert a syntect highlighting style to a ratatui Style.
fn syntect_style_to_ratatui(style: syntect::highlighting::Style) -> Style {
    let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);

    let mut ratatui_style = Style::default().fg(fg);

    if style.font_style.contains(FontStyle::BOLD) {
        ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        ratatui_style = ratatui_style.add_modifier(Modifier::UNDERLINED);
    }

    ratatui_style
}
