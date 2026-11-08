use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub underline: bool,
    pub sup: bool,
    pub sub: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inline {
    pub text: String,
    pub style: InlineStyle,
    /// Absolute URL for links, validated during extraction.
    pub link: Option<String>,
}

impl Inline {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: InlineStyle::default(),
            link: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableBlock {
    pub caption: Option<Vec<Inline>>,
    pub headers: Vec<Vec<Inline>>,
    pub rows: Vec<Vec<Vec<Inline>>>,
    pub aligns: Vec<ColumnAlign>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageBlock {
    /// Stable hash of the resolved URL.
    pub id: String,
    pub src: String,
    pub alt: String,
    pub title: Option<String>,
    pub width_px: Option<u32>,
    pub height_px: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListItem {
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Block {
    Heading {
        level: u8,
        spans: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    List {
        ordered: bool,
        start: u64,
        items: Vec<ListItem>,
    },
    Quote(Vec<Block>),
    Code {
        language: Option<String>,
        text: String,
    },
    Table(TableBlock),
    Image(ImageBlock),
    Figure {
        image: ImageBlock,
        caption: Vec<Inline>,
    },
    Divider,
}

impl Block {
    pub fn plain_text(&self) -> String {
        match self {
            Block::Heading { spans, .. } | Block::Paragraph(spans) => {
                spans.iter().map(|s| s.text.as_str()).collect()
            }
            Block::List { items, .. } => items
                .iter()
                .flat_map(|i| i.blocks.iter().map(|b| b.plain_text()))
                .collect::<Vec<_>>()
                .join(" "),
            Block::Quote(blocks) => blocks
                .iter()
                .map(|b| b.plain_text())
                .collect::<Vec<_>>()
                .join(" "),
            Block::Code { text, .. } => text.clone(),
            Block::Table(t) => t
                .rows
                .iter()
                .flatten()
                .flatten()
                .map(|i| i.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            Block::Image(img) => img.alt.clone(),
            Block::Figure { caption, image } => {
                let cap = caption.iter().map(|i| i.text.as_str()).collect::<String>();
                if cap.is_empty() {
                    image.alt.clone()
                } else {
                    cap
                }
            }
            Block::Divider => String::new(),
        }
    }

    pub fn word_count(&self) -> usize {
        self.plain_text().split_whitespace().count()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TocEntry {
    pub block_index: usize,
    pub level: u8,
    pub title: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtractSource {
    Readability,
    Heuristic,
    JsonLd,
    PlainText,
    Cache,
}

impl ExtractSource {
    pub fn label(self) -> &'static str {
        match self {
            ExtractSource::Readability => "readability",
            ExtractSource::Heuristic => "heuristic",
            ExtractSource::JsonLd => "json-ld",
            ExtractSource::PlainText => "plain text",
            ExtractSource::Cache => "cache",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Article {
    pub requested_url: String,
    pub final_url: String,
    pub title: String,
    pub byline: Option<String>,
    pub site_name: Option<String>,
    /// Unix timestamp in seconds.
    pub published_at: Option<i64>,
    pub lang: Option<String>,
    pub blocks: Vec<Block>,
    pub toc: Vec<TocEntry>,
    pub anchors: HashMap<String, usize>,
    pub word_count: usize,
    pub image_count: usize,
    pub source: ExtractSource,
    pub warnings: Vec<String>,
}

impl Article {
    pub fn reading_minutes(&self) -> usize {
        crate::util::time::reading_minutes(self.word_count)
    }

    /// All links in document order, deduplicated by (url, text).
    pub fn links(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut push_spans = |spans: &[Inline]| {
            for s in spans {
                if let Some(url) = &s.link {
                    let text = s.text.trim().to_string();
                    if !text.is_empty() && !out.iter().any(|(u, t)| u == url && *t == text) {
                        out.push((url.clone(), text));
                    }
                }
            }
        };
        for block in &self.blocks {
            match block {
                Block::Heading { spans, .. } | Block::Paragraph(spans) => push_spans(spans),
                Block::List { items, .. } => {
                    for item in items {
                        for b in &item.blocks {
                            match b {
                                Block::Paragraph(spans) | Block::Heading { spans, .. } => {
                                    push_spans(spans)
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Block::Quote(blocks) => {
                    for b in blocks {
                        if let Block::Paragraph(spans) = b {
                            push_spans(spans);
                        }
                    }
                }
                Block::Table(t) => {
                    for cell in &t.headers {
                        push_spans(cell);
                    }
                    for row in &t.rows {
                        for cell in row {
                            push_spans(cell);
                        }
                    }
                }
                Block::Figure { caption, .. } => push_spans(caption),
                _ => {}
            }
        }
        out
    }
}
