use ratatui::style::Style;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::style::Wrap;

/// A paragraph of styled text, the content of a `<text>` element or of a bare text node.
#[derive(Debug, Clone, Default)]
pub(crate) struct Paragraph {
    pub(crate) spans: Vec<(String, Style)>,
    pub(crate) wrap: Wrap,
}

#[derive(Debug, Clone, Copy)]
struct Grapheme<'a> {
    symbol: &'a str,
    width: usize,
    style: Style,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TokenKind {
    Word,
    Space,
    Newline,
}

struct Token<'a> {
    kind: TokenKind,
    graphemes: Vec<Grapheme<'a>>,
}

impl Token<'_> {
    fn width(&self) -> usize {
        self.graphemes.iter().map(|grapheme| grapheme.width).sum()
    }
}

/// One line of laid out text
#[derive(Debug, Default)]
pub(crate) struct WrappedLine<'a> {
    graphemes: Vec<Grapheme<'a>>,
}

impl<'a> WrappedLine<'a> {
    pub(crate) fn width(&self) -> usize {
        self.graphemes.iter().map(|grapheme| grapheme.width).sum()
    }

    /// The symbols of the line with their width and style
    pub(crate) fn graphemes(&self) -> impl Iterator<Item = (&'a str, usize, Style)> + '_ {
        self.graphemes
            .iter()
            .map(|grapheme| (grapheme.symbol, grapheme.width, grapheme.style))
    }

    fn push(&mut self, grapheme: Grapheme<'a>) {
        self.graphemes.push(grapheme);
    }

    fn trim_end(&mut self) {
        while self
            .graphemes
            .last()
            .is_some_and(|grapheme| grapheme.symbol.trim().is_empty())
        {
            self.graphemes.pop();
        }
    }

    /// Merge graphemes of the same style into spans
    #[cfg(test)]
    pub(crate) fn to_line(&self) -> ratatui::text::Line<'a> {
        use ratatui::text::{Line, Span};

        let mut spans: Vec<Span<'a>> = Vec::new();
        let mut current = String::new();
        let mut current_style = None;
        for grapheme in &self.graphemes {
            if current_style != Some(grapheme.style) {
                if let Some(style) = current_style {
                    spans.push(Span::styled(std::mem::take(&mut current), style));
                }
                current_style = Some(grapheme.style);
            }
            current.push_str(grapheme.symbol);
        }
        if let Some(style) = current_style {
            spans.push(Span::styled(current, style));
        }
        Line::from(spans)
    }
}

impl Paragraph {
    fn tokens(&self) -> Vec<Token<'_>> {
        let mut tokens: Vec<Token> = Vec::new();
        for (text, style) in &self.spans {
            for symbol in text.graphemes(true) {
                let kind = if symbol == "\n" || symbol == "\r\n" {
                    TokenKind::Newline
                } else if symbol.trim().is_empty() {
                    TokenKind::Space
                } else {
                    TokenKind::Word
                };
                let grapheme = Grapheme {
                    symbol: if symbol == "\t" { "    " } else { symbol },
                    width: if symbol == "\t" {
                        4
                    } else if kind == TokenKind::Newline {
                        0
                    } else {
                        symbol.width()
                    },
                    style: *style,
                };
                // Zero width graphemes (e.g. control characters) would break the cell grid
                if kind != TokenKind::Newline && grapheme.width == 0 {
                    continue;
                }
                match tokens.last_mut() {
                    Some(token) if token.kind == kind && kind != TokenKind::Newline => {
                        token.graphemes.push(grapheme)
                    }
                    _ => tokens.push(Token {
                        kind,
                        graphemes: vec![grapheme],
                    }),
                }
            }
        }
        tokens
    }

    /// Lay out the paragraph into lines no wider than `max_width` (if wrapping is enabled)
    pub(crate) fn wrap(&self, max_width: Option<usize>) -> Vec<WrappedLine<'_>> {
        let max_width = match self.wrap {
            Wrap::None => None,
            _ => max_width.map(|width| width.max(1)),
        };
        let mut lines = Vec::new();
        let mut line = WrappedLine::default();
        let mut line_width = 0;
        let mut soft_wrapped = false;

        for token in self.tokens() {
            match token.kind {
                TokenKind::Newline => {
                    lines.push(std::mem::take(&mut line));
                    line_width = 0;
                    soft_wrapped = false;
                }
                TokenKind::Space if self.wrap == Wrap::Word => {
                    // Whitespace at the start of a wrapped line is dropped
                    if soft_wrapped && line_width == 0 {
                        continue;
                    }
                    for grapheme in token.graphemes {
                        if max_width.is_some_and(|max| line_width + grapheme.width > max) {
                            line.trim_end();
                            lines.push(std::mem::take(&mut line));
                            line_width = 0;
                            soft_wrapped = true;
                            break;
                        }
                        line_width += grapheme.width;
                        line.push(grapheme);
                    }
                }
                _ => {
                    let token_width = token.width();
                    if self.wrap == Wrap::Word
                        && line_width > 0
                        && max_width.is_some_and(|max| line_width + token_width > max)
                    {
                        line.trim_end();
                        lines.push(std::mem::take(&mut line));
                        line_width = 0;
                        soft_wrapped = true;
                    }
                    for grapheme in token.graphemes {
                        if line_width > 0
                            && max_width.is_some_and(|max| line_width + grapheme.width > max)
                        {
                            lines.push(std::mem::take(&mut line));
                            line_width = 0;
                            soft_wrapped = true;
                        }
                        line_width += grapheme.width;
                        line.push(grapheme);
                    }
                }
            }
        }
        if !line.graphemes.is_empty() || !lines.is_empty() {
            lines.push(line);
        }
        lines
    }

    /// The narrowest width the paragraph can be laid out in without overflowing
    pub(crate) fn min_content_width(&self) -> usize {
        match self.wrap {
            Wrap::None => self.max_content_width(),
            Wrap::Char => self
                .tokens()
                .iter()
                .flat_map(|token| token.graphemes.iter().map(|grapheme| grapheme.width))
                .max()
                .unwrap_or(0),
            Wrap::Word => self
                .tokens()
                .iter()
                .filter(|token| token.kind == TokenKind::Word)
                .map(Token::width)
                .max()
                .unwrap_or(0),
        }
    }

    pub(crate) fn max_content_width(&self) -> usize {
        self.wrap(None)
            .iter()
            .map(WrappedLine::width)
            .max()
            .unwrap_or(0)
    }

    /// Size of the paragraph when laid out within `max_width`
    pub(crate) fn measure(&self, max_width: Option<usize>) -> (usize, usize) {
        let lines = self.wrap(max_width);
        let width = lines.iter().map(WrappedLine::width).max().unwrap_or(0);
        (width, lines.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraph(text: &str, wrap: Wrap) -> Paragraph {
        Paragraph {
            spans: vec![(text.to_string(), Style::default())],
            wrap,
        }
    }

    fn lines(paragraph: &Paragraph, width: Option<usize>) -> Vec<String> {
        paragraph
            .wrap(width)
            .iter()
            .map(|line| line.to_line().to_string())
            .collect()
    }

    #[test]
    fn word_wrap() {
        let text = paragraph("hello wide world", Wrap::Word);
        assert_eq!(lines(&text, Some(10)), vec!["hello wide", "world"]);
        assert_eq!(lines(&text, Some(7)), vec!["hello", "wide", "world"]);
        assert_eq!(lines(&text, None), vec!["hello wide world"]);
    }

    #[test]
    fn breaks_long_words() {
        let text = paragraph("abcdefgh ij", Wrap::Word);
        assert_eq!(lines(&text, Some(3)), vec!["abc", "def", "gh", "ij"]);
    }

    #[test]
    fn char_wrap_and_newlines() {
        let text = paragraph("abcd\nef", Wrap::Char);
        assert_eq!(lines(&text, Some(3)), vec!["abc", "d", "ef"]);
    }

    #[test]
    fn wide_characters() {
        let text = paragraph("日本語テキスト", Wrap::Char);
        assert_eq!(lines(&text, Some(6)), vec!["日本語", "テキス", "ト"]);
        assert_eq!(text.max_content_width(), 14);
    }

    #[test]
    fn min_content_width() {
        assert_eq!(paragraph("a bbb cc", Wrap::Word).min_content_width(), 3);
        assert_eq!(paragraph("a bbb cc", Wrap::None).min_content_width(), 8);
    }

    #[test]
    fn keeps_styles() {
        let text = Paragraph {
            spans: vec![
                ("ab".to_string(), Style::new().bold()),
                ("cd".to_string(), Style::new()),
            ],
            wrap: Wrap::Word,
        };
        let line = text.wrap(None).remove(0).to_line();
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content, "ab");
    }
}
