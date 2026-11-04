use std::{collections::HashMap, str::FromStr};

use ratatui::{
    style::{Color, Modifier, Style},
    widgets::BorderType,
};
use taffy::{
    AlignItems, AlignSelf, Dimension, Display, FlexDirection, FlexWrap, JustifyContent,
    LengthPercentage, LengthPercentageAuto, Overflow, Point, Position, Rect,
};

/// How an element takes part in layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    /// A flex container (`<box>`, `<row>`, `<column>`, ...)
    Box,
    /// A paragraph (`<text>`, `<span>`, `<p>`): all descendants are flattened into styled text
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum Wrap {
    #[default]
    Word,
    Char,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// A taffy style that can be sent between threads.
///
/// `taffy::Style` is not `Send` because lengths can hold a pointer to a `calc()` expression.
/// Styles are only ever built by [ElementStyle::parse] which never creates `calc()` lengths,
/// so the style holds plain numbers only.
#[derive(Debug, Clone, Default)]
pub(crate) struct LayoutStyle(pub(crate) taffy::Style);

// SAFETY: see above, a LayoutStyle never contains `calc()` pointers.
unsafe impl Send for LayoutStyle {}
// SAFETY: see above, a LayoutStyle never contains `calc()` pointers.
unsafe impl Sync for LayoutStyle {}

impl std::ops::Deref for LayoutStyle {
    type Target = taffy::Style;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The parsed attributes of an element.
#[derive(Debug, Clone)]
pub(crate) struct ElementStyle {
    pub(crate) kind: Kind,
    pub(crate) layout: LayoutStyle,
    /// Text style, inherited by all descendants
    pub(crate) text: Style,
    /// Background fill for the whole area of a box
    pub(crate) background: Option<Color>,
    pub(crate) border: Option<BorderType>,
    pub(crate) border_style: Style,
    pub(crate) title: Option<String>,
    pub(crate) wrap: Wrap,
    pub(crate) align: Align,
    pub(crate) clip: bool,
}

impl ElementStyle {
    pub(crate) fn parse(tag_name: &str, attributes: &HashMap<String, String>) -> Self {
        let kind = match tag_name {
            "text" | "span" | "p" => Kind::Text,
            _ => Kind::Box,
        };

        let mut layout = taffy::Style {
            flex_direction: match tag_name {
                "column" | "col" => FlexDirection::Column,
                _ => FlexDirection::Row,
            },
            // Terminal UIs usually want content to keep its size rather than being squished
            flex_shrink: if kind == Kind::Text { 1.0 } else { 0.0 },
            ..Default::default()
        };
        let mut text = Style::default();
        let mut background = None;
        let mut border = None;
        let mut border_style = Style::default();
        let mut title = None;
        let mut wrap = Wrap::default();
        let mut align = Align::default();
        let mut clip = false;

        // Apply shorthands first so the specific attributes (e.g. `padding_left`) win
        let mut attributes: Vec<_> = attributes.iter().collect();
        attributes.sort_by_key(|(key, _)| (specificity(key), key.as_str()));

        for (key, raw_value) in attributes {
            let value = raw_value.trim();
            match key.as_str() {
                "display" => {
                    layout.display = match value {
                        "none" => Display::None,
                        _ => Display::Flex,
                    }
                }
                "direction" | "flex_direction" => {
                    if let Some(direction) = parse_direction(value) {
                        layout.flex_direction = direction;
                    }
                }
                "wrap" => {
                    // `wrap` configures text wrapping on text elements and flex wrapping on boxes
                    if kind == Kind::Text {
                        wrap = match value {
                            "char" => Wrap::Char,
                            "none" | "false" => Wrap::None,
                            _ => Wrap::Word,
                        };
                    } else {
                        layout.flex_wrap = parse_flex_wrap(value);
                    }
                }
                "flex_wrap" => layout.flex_wrap = parse_flex_wrap(value),
                "grow" | "flex_grow" => layout.flex_grow = parse_number(value),
                "shrink" | "flex_shrink" => layout.flex_shrink = parse_number(value),
                "basis" | "flex_basis" => layout.flex_basis = parse_dimension(value),
                "width" => layout.size.width = parse_dimension(value),
                "height" => layout.size.height = parse_dimension(value),
                "min_width" => layout.min_size.width = parse_length_percentage_auto(value),
                "min_height" => layout.min_size.height = parse_length_percentage_auto(value),
                "max_width" => layout.max_size.width = parse_length_percentage_auto(value),
                "max_height" => layout.max_size.height = parse_length_percentage_auto(value),
                "padding" => layout.padding = parse_edges(value, parse_length_percentage),
                "padding_x" => {
                    let padding = parse_length_percentage(value);
                    layout.padding.left = padding;
                    layout.padding.right = padding;
                }
                "padding_y" => {
                    let padding = parse_length_percentage(value);
                    layout.padding.top = padding;
                    layout.padding.bottom = padding;
                }
                "padding_top" => layout.padding.top = parse_length_percentage(value),
                "padding_right" => layout.padding.right = parse_length_percentage(value),
                "padding_bottom" => layout.padding.bottom = parse_length_percentage(value),
                "padding_left" => layout.padding.left = parse_length_percentage(value),
                "margin" => layout.margin = parse_edges(value, parse_length_percentage_auto),
                "margin_x" => {
                    let margin = parse_length_percentage_auto(value);
                    layout.margin.left = margin;
                    layout.margin.right = margin;
                }
                "margin_y" => {
                    let margin = parse_length_percentage_auto(value);
                    layout.margin.top = margin;
                    layout.margin.bottom = margin;
                }
                "margin_top" => layout.margin.top = parse_length_percentage_auto(value),
                "margin_right" => layout.margin.right = parse_length_percentage_auto(value),
                "margin_bottom" => layout.margin.bottom = parse_length_percentage_auto(value),
                "margin_left" => layout.margin.left = parse_length_percentage_auto(value),
                "gap" => {
                    let gap = parse_length_percentage(value);
                    layout.gap = taffy::Size {
                        width: gap,
                        height: gap,
                    };
                }
                "row_gap" => layout.gap.height = parse_length_percentage(value),
                "column_gap" => layout.gap.width = parse_length_percentage(value),
                "justify_content" | "justify" => {
                    if let Some(justify_content) = parse_justify_content(value) {
                        layout.justify_content = justify_content;
                    }
                }
                "align_items" | "align" => {
                    if let Some(align_items) = parse_align_items(value) {
                        layout.align_items = align_items;
                    }
                }
                "align_self" => layout.align_self = parse_align_items(value),
                "position" => {
                    layout.position = match value {
                        "absolute" => Position::Absolute,
                        _ => Position::Relative,
                    }
                }
                "top" => layout.inset.top = parse_length_percentage_auto(value),
                "right" => layout.inset.right = parse_length_percentage_auto(value),
                "bottom" => layout.inset.bottom = parse_length_percentage_auto(value),
                "left" => layout.inset.left = parse_length_percentage_auto(value),
                "overflow" => {
                    clip = value == "hidden";
                    let overflow = if clip {
                        Overflow::Hidden
                    } else {
                        Overflow::Visible
                    };
                    layout.overflow = Point {
                        x: overflow,
                        y: overflow,
                    };
                }
                "border" => {
                    border = match value {
                        "none" | "false" => None,
                        "rounded" => Some(BorderType::Rounded),
                        "double" => Some(BorderType::Double),
                        "thick" => Some(BorderType::Thick),
                        _ => Some(BorderType::Plain),
                    }
                }
                "border_color" => border_style.fg = parse_color(value),
                "title" => title = Some(raw_value.clone()),
                "color" | "fg" => text.fg = parse_color(value),
                "bg" | "background" => {
                    background = parse_color(value);
                    text.bg = background;
                }
                "bold" => text = toggle(text, Modifier::BOLD, value),
                "dim" => text = toggle(text, Modifier::DIM, value),
                "italic" => text = toggle(text, Modifier::ITALIC, value),
                "underline" | "underlined" => text = toggle(text, Modifier::UNDERLINED, value),
                "reversed" | "inverse" => text = toggle(text, Modifier::REVERSED, value),
                "strikethrough" => text = toggle(text, Modifier::CROSSED_OUT, value),
                "text_align" => {
                    align = match value {
                        "center" => Align::Center,
                        "right" | "end" => Align::Right,
                        _ => Align::Left,
                    }
                }
                _ => {}
            }
        }

        if border.is_some() {
            layout.border = Rect {
                left: LengthPercentage::length(1.0),
                right: LengthPercentage::length(1.0),
                top: LengthPercentage::length(1.0),
                bottom: LengthPercentage::length(1.0),
            };
        }

        Self {
            kind,
            layout: LayoutStyle(layout),
            text,
            background,
            border,
            border_style,
            title,
            wrap,
            align,
            clip,
        }
    }
}

fn specificity(key: &str) -> u8 {
    match key {
        "padding" | "margin" | "gap" => 0,
        "padding_x" | "padding_y" | "margin_x" | "margin_y" => 1,
        _ => 2,
    }
}

fn toggle(style: Style, modifier: Modifier, value: &str) -> Style {
    if value == "false" {
        style.remove_modifier(modifier)
    } else {
        style.add_modifier(modifier)
    }
}

fn parse_number(value: &str) -> f32 {
    value.parse().unwrap_or(0.0)
}

fn parse_color(value: &str) -> Option<Color> {
    Color::from_str(value).ok()
}

fn parse_direction(value: &str) -> Option<FlexDirection> {
    match value {
        "row" => Some(FlexDirection::Row),
        "column" | "col" => Some(FlexDirection::Column),
        "row-reverse" | "row_reverse" => Some(FlexDirection::RowReverse),
        "column-reverse" | "column_reverse" => Some(FlexDirection::ColumnReverse),
        _ => None,
    }
}

fn parse_flex_wrap(value: &str) -> FlexWrap {
    match value {
        "wrap" | "true" => FlexWrap::Wrap,
        "wrap-reverse" | "wrap_reverse" => FlexWrap::WrapReverse,
        _ => FlexWrap::NoWrap,
    }
}

fn parse_justify_content(value: &str) -> Option<JustifyContent> {
    match value {
        "start" | "flex-start" | "flex_start" => Some(JustifyContent::FLEX_START),
        "end" | "flex-end" | "flex_end" => Some(JustifyContent::FLEX_END),
        "center" => Some(JustifyContent::CENTER),
        "space-between" | "space_between" => Some(JustifyContent::SPACE_BETWEEN),
        "space-around" | "space_around" => Some(JustifyContent::SPACE_AROUND),
        "space-evenly" | "space_evenly" => Some(JustifyContent::SPACE_EVENLY),
        _ => None,
    }
}

fn parse_align_items(value: &str) -> Option<AlignItems> {
    match value {
        "start" | "flex-start" | "flex_start" => Some(AlignSelf::FLEX_START),
        "end" | "flex-end" | "flex_end" => Some(AlignSelf::FLEX_END),
        "center" => Some(AlignSelf::CENTER),
        "stretch" => Some(AlignSelf::STRETCH),
        "baseline" => Some(AlignSelf::BASELINE),
        _ => None,
    }
}

fn parse_dimension(value: &str) -> Dimension {
    if let Some(percent) = value.strip_suffix('%') {
        Dimension::percent(parse_number(percent) / 100.0)
    } else if let Ok(length) = value.parse() {
        Dimension::length(length)
    } else {
        Dimension::auto()
    }
}

fn parse_length_percentage(value: &str) -> LengthPercentage {
    match value.strip_suffix('%') {
        Some(percent) => LengthPercentage::percent(parse_number(percent) / 100.0),
        None => LengthPercentage::length(parse_number(value)),
    }
}

fn parse_length_percentage_auto(value: &str) -> LengthPercentageAuto {
    match value {
        "auto" => LengthPercentageAuto::auto(),
        _ => parse_length_percentage(value).into(),
    }
}

/// Parse CSS-like shorthands: `1`, `1 2` (vertical horizontal), `1 2 3` or `1 2 3 4` (top right bottom left)
fn parse_edges<T: Copy>(value: &str, parse: fn(&str) -> T) -> Rect<T> {
    let values: Vec<T> = value.split_whitespace().map(parse).collect();
    let (top, right, bottom, left) = match values.as_slice() {
        [all] => (*all, *all, *all, *all),
        [vertical, horizontal] => (*vertical, *horizontal, *vertical, *horizontal),
        [top, horizontal, bottom] => (*top, *horizontal, *bottom, *horizontal),
        [top, right, bottom, left, ..] => (*top, *right, *bottom, *left),
        [] => {
            let zero = parse("0");
            (zero, zero, zero, zero)
        }
    };
    Rect {
        top,
        right,
        bottom,
        left,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(tag_name: &str, attributes: &[(&str, &str)]) -> ElementStyle {
        ElementStyle::parse(
            tag_name,
            &attributes
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        )
    }

    #[test]
    fn padding_shorthand_and_override() {
        let style = parse("box", &[("padding", "1 2"), ("padding_left", "5")]);
        assert_eq!(style.layout.padding.top, LengthPercentage::length(1.0));
        assert_eq!(style.layout.padding.right, LengthPercentage::length(2.0));
        assert_eq!(style.layout.padding.left, LengthPercentage::length(5.0));
    }

    #[test]
    fn column_and_text_kinds() {
        assert_eq!(
            parse("column", &[]).layout.flex_direction,
            FlexDirection::Column
        );
        assert_eq!(parse("text", &[]).kind, Kind::Text);
        assert_eq!(parse("box", &[]).kind, Kind::Box);
    }

    #[test]
    fn text_style() {
        let style = parse(
            "text",
            &[("bold", "true"), ("color", "red"), ("bg", "#00ff00")],
        );
        assert_eq!(style.text.fg, Some(Color::Red));
        assert_eq!(style.text.bg, Some(Color::Rgb(0, 255, 0)));
        assert!(style.text.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn border_adds_layout_border() {
        let style = parse("box", &[("border", "rounded")]);
        assert_eq!(style.border, Some(BorderType::Rounded));
        assert_eq!(style.layout.border.top, LengthPercentage::length(1.0));
    }
}
