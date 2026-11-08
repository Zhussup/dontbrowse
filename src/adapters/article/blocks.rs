use std::collections::HashMap;

use scraper::{ElementRef, Html, Selector};
use sha2::{Digest, Sha256};

use super::sanitize::{largest_srcset, resolve_url};
use crate::model::{
    Block, ColumnAlign, ImageBlock, Inline, InlineStyle, ListItem, TableBlock, TocEntry,
};
use crate::util::text::{collapse_whitespace, sanitize_text};
use url::Url;

pub struct MapResult {
    pub blocks: Vec<Block>,
    pub toc: Vec<TocEntry>,
    pub anchors: HashMap<String, usize>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Default)]
struct Ctx {
    style: InlineStyle,
    link: Option<String>,
}

#[derive(Default)]
struct InlineAcc {
    spans: Vec<Inline>,
    buf: String,
    style: InlineStyle,
    link: Option<String>,
}

impl InlineAcc {
    fn push_text(&mut self, text: &str, ctx: &Ctx) {
        let clean = sanitize_text(text).replace(['\n', '\r', '\t'], " ");
        self.push_raw(&clean, ctx);
    }

    fn push_hard_break(&mut self, ctx: &Ctx) {
        self.push_raw("\n", ctx);
    }

    fn push_raw(&mut self, text: &str, ctx: &Ctx) {
        if self.style != ctx.style || self.link != ctx.link {
            self.flush();
            self.style = ctx.style;
            self.link = ctx.link.clone();
        }
        self.buf.push_str(text);
    }

    fn flush(&mut self) {
        if !self.buf.is_empty() {
            let text = std::mem::take(&mut self.buf);
            self.spans.push(Inline {
                text,
                style: self.style,
                link: self.link.clone(),
            });
        }
    }

    fn finish(mut self) -> Vec<Inline> {
        self.flush();
        normalize_spans(self.spans)
    }
}

fn normalize_spans(mut spans: Vec<Inline>) -> Vec<Inline> {
    if let Some(first) = spans.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
    if let Some(last) = spans.last_mut() {
        last.text = last.text.trim_end().to_string();
    }
    let mut out: Vec<Inline> = Vec::new();
    for span in spans {
        if span.text.is_empty() {
            continue;
        }
        if let Some(last) = out.last_mut()
            && last.style == span.style
            && last.link == span.link
        {
            last.text.push_str(&span.text);
            continue;
        }
        out.push(span);
    }
    out
}

fn flush_inline(acc: &mut InlineAcc, out: &mut Vec<Block>) {
    let spans = std::mem::take(acc).finish();
    if !spans.is_empty() {
        out.push(Block::Paragraph(spans));
    }
}

pub fn map_fragment(html: &str, base_url: &str) -> MapResult {
    let doc = Html::parse_fragment(html);
    let root = doc.root_element();
    map_root(root, base_url)
}

pub fn map_document(html: &str, base_url: &str) -> MapResult {
    let doc = Html::parse_document(html);
    let root = if let Some(selector) = selector("body") {
        doc.select(&selector)
            .next()
            .unwrap_or_else(|| doc.root_element())
    } else {
        doc.root_element()
    };
    map_root(root, base_url)
}

pub fn map_element(element: ElementRef<'_>, base_url: &str) -> MapResult {
    let mut mapper = Mapper::new(base_url);
    let mut blocks = Vec::new();
    mapper.map_children(element, &mut blocks, &Ctx::default(), true);
    MapResult {
        blocks,
        toc: mapper.toc,
        anchors: mapper.anchors,
        warnings: mapper.warnings,
    }
}

fn map_root(root: ElementRef<'_>, base_url: &str) -> MapResult {
    map_element(root, base_url)
}

struct Mapper {
    base: Url,
    toc: Vec<TocEntry>,
    anchors: HashMap<String, usize>,
    warnings: Vec<String>,
}

impl Mapper {
    fn new(base_url: &str) -> Self {
        Self {
            base: Url::parse(base_url)
                .or_else(|_| Url::parse("https://example.invalid/"))
                .expect("static fallback url"),
            toc: Vec::new(),
            anchors: HashMap::new(),
            warnings: Vec::new(),
        }
    }

    fn map_children(
        &mut self,
        parent: ElementRef<'_>,
        out: &mut Vec<Block>,
        ctx: &Ctx,
        top_level: bool,
    ) {
        let mut inline = InlineAcc::default();
        for child in parent.children() {
            if child.value().is_text() {
                if let Some(text) = child.value().as_text() {
                    inline.push_text(&text.text, ctx);
                }
            } else if let Some(element) = ElementRef::wrap(child) {
                if should_skip(&element) {
                    continue;
                }
                let name = element.value().name();
                if is_block(name) {
                    flush_inline(&mut inline, out);
                    self.map_block(element, out, ctx, top_level);
                } else {
                    self.collect_inline(element, &mut inline, ctx);
                }
            }
        }
        flush_inline(&mut inline, out);
    }

    fn map_block(
        &mut self,
        element: ElementRef<'_>,
        out: &mut Vec<Block>,
        ctx: &Ctx,
        top_level: bool,
    ) {
        if should_skip(&element) {
            return;
        }
        let name = element.value().name();
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name[1..].parse::<u8>().unwrap_or(2);
                let spans = self.collect_inline_element(element, ctx);
                if spans.is_empty() {
                    return;
                }
                let index = out.len();
                if top_level {
                    if let Some(id) = element.attr("id")
                        && !id.is_empty()
                    {
                        self.anchors.insert(id.to_string(), index);
                    }
                    if level <= 4 {
                        let title = collapse_whitespace(
                            &spans.iter().map(|s| s.text.as_str()).collect::<String>(),
                        );
                        if !title.is_empty() {
                            self.toc.push(TocEntry {
                                block_index: index,
                                level,
                                title,
                            });
                        }
                    }
                }
                out.push(Block::Heading { level, spans });
            }
            "p" => {
                let spans = self.collect_inline_element(element, ctx);
                if !spans.is_empty() {
                    out.push(Block::Paragraph(spans));
                }
            }
            "ul" | "ol" => self.map_list(element, name == "ol", out, ctx),
            "blockquote" => {
                let mut inner = Vec::new();
                self.map_children(element, &mut inner, ctx, false);
                if !inner.is_empty() {
                    out.push(Block::Quote(inner));
                }
            }
            "pre" => out.push(self.extract_code(element)),
            "table" => {
                if let Some(table) = self.map_table(element) {
                    out.push(Block::Table(table));
                }
            }
            "figure" => self.map_figure(element, out, ctx),
            "img" => {
                if let Some(image) = self.map_image(element) {
                    out.push(Block::Image(image));
                }
            }
            "hr" => out.push(Block::Divider),
            "li" => self.map_children(element, out, ctx, top_level),
            _ => self.map_children(element, out, ctx, top_level),
        }
    }

    fn map_list(
        &mut self,
        element: ElementRef<'_>,
        ordered: bool,
        out: &mut Vec<Block>,
        ctx: &Ctx,
    ) {
        let start = element
            .attr("start")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(1);
        let mut items = Vec::new();
        for child in element.children() {
            if let Some(li) = ElementRef::wrap(child) {
                if should_skip(&li) {
                    continue;
                }
                if li.value().name() != "li" {
                    continue;
                }
                let mut blocks = Vec::new();
                self.map_children(li, &mut blocks, ctx, false);
                if !blocks.is_empty() {
                    items.push(ListItem { blocks });
                }
            }
        }
        if !items.is_empty() {
            out.push(Block::List {
                ordered,
                start,
                items,
            });
        }
    }

    fn collect_inline_element(&mut self, element: ElementRef<'_>, ctx: &Ctx) -> Vec<Inline> {
        let mut acc = InlineAcc::default();
        self.collect_inline(element, &mut acc, ctx);
        acc.finish()
    }

    fn collect_inline(&mut self, element: ElementRef<'_>, acc: &mut InlineAcc, ctx: &Ctx) {
        if should_skip(&element) {
            return;
        }
        let name = element.value().name();
        if name == "br" {
            acc.push_hard_break(ctx);
            return;
        }
        if name == "img" {
            if let Some(alt) = element.attr("alt") {
                let alt = sanitize_text(alt);
                if !alt.trim().is_empty() {
                    acc.push_text(&alt, ctx);
                }
            }
            return;
        }

        let mut next = ctx.clone();
        match name {
            "a" => {
                if let Some(href) = element.attr("href")
                    && let Some(url) = resolve_url(&self.base, href)
                {
                    next.link = Some(url);
                }
            }
            "strong" | "b" => next.style.bold = true,
            "em" | "i" | "cite" => next.style.italic = true,
            "code" | "kbd" | "samp" | "var" => next.style.code = true,
            "del" | "s" | "strike" => next.style.strike = true,
            "u" | "ins" => next.style.underline = true,
            "sup" => next.style.sup = true,
            "sub" => next.style.sub = true,
            "mark" => next.style.bold = true,
            _ => {}
        }

        for child in element.children() {
            if child.value().is_text() {
                if let Some(text) = child.value().as_text() {
                    acc.push_text(&text.text, &next);
                }
            } else if let Some(child_element) = ElementRef::wrap(child) {
                let child_is_block = is_block(child_element.value().name());
                if child_is_block {
                    acc.push_text(" ", &next);
                }
                self.collect_inline(child_element, acc, &next);
                if child_is_block {
                    acc.push_text(" ", &next);
                }
            }
        }
    }

    fn map_figure(&mut self, element: ElementRef<'_>, out: &mut Vec<Block>, ctx: &Ctx) {
        let mut image = None;
        let mut caption = Vec::new();
        for child in element.children() {
            if let Some(child_element) = ElementRef::wrap(child) {
                match child_element.value().name() {
                    "img" if image.is_none() => image = self.map_image(child_element),
                    "figcaption" => {
                        caption = self.collect_inline_element(child_element, ctx);
                    }
                    _ => {}
                }
            }
        }
        if image.is_none()
            && let Some(img) = element
                .descendent_elements()
                .find(|e| e.value().name() == "img")
        {
            image = self.map_image(img);
        }
        match image {
            Some(image) => out.push(Block::Figure { image, caption }),
            None => self.map_children(element, out, ctx, false),
        }
    }

    fn map_image(&self, element: ElementRef<'_>) -> Option<ImageBlock> {
        let raw = element
            .attr("srcset")
            .and_then(largest_srcset)
            .or_else(|| element.attr("data-src").map(str::to_string))
            .or_else(|| element.attr("data-original").map(str::to_string))
            .or_else(|| element.attr("src").map(str::to_string))?;

        let src = resolve_url(&self.base, &raw)?;
        if !(src.starts_with("http://") || src.starts_with("https://")) {
            return None;
        }
        let width_px = element.attr("width").and_then(parse_dim);
        let height_px = element.attr("height").and_then(parse_dim);
        if width_px.unwrap_or(10) <= 2 && height_px.unwrap_or(10) <= 2 {
            return None;
        }

        let mut hasher = Sha256::new();
        hasher.update(src.as_bytes());
        let id = hex::encode(&hasher.finalize()[..8]);

        Some(ImageBlock {
            id,
            src,
            alt: collapse_whitespace(&sanitize_text(element.attr("alt").unwrap_or(""))),
            title: element.attr("title").map(sanitize_text),
            width_px,
            height_px,
        })
    }

    fn extract_code(&self, element: ElementRef<'_>) -> Block {
        let code_element = if element.value().name() == "code" {
            Some(element)
        } else {
            element
                .descendent_elements()
                .find(|e| e.value().name() == "code")
        };

        let raw = code_element
            .map(|c| c.text().collect::<String>())
            .unwrap_or_else(|| element.text().collect::<String>());
        let text = raw.trim_matches('\n').to_string();

        let language = code_element
            .and_then(|c| c.attr("class"))
            .or_else(|| element.attr("class"))
            .and_then(|classes| {
                classes.split_whitespace().find_map(|class| {
                    class
                        .strip_prefix("language-")
                        .or_else(|| class.strip_prefix("lang-"))
                        .map(|s| s.to_string())
                })
            })
            .or_else(|| element.attr("data-language").map(str::to_string));

        Block::Code {
            language: language.map(|l| l.chars().take(16).collect()),
            text,
        }
    }

    fn map_table(&mut self, element: ElementRef<'_>) -> Option<TableBlock> {
        let caption = element
            .children()
            .filter_map(ElementRef::wrap)
            .find(|e| e.value().name() == "caption")
            .map(|e| self.collect_inline_element(e, &Ctx::default()));

        let rows: Vec<ElementRef> = element
            .descendent_elements()
            .filter(|e| e.value().name() == "tr")
            .collect();
        if rows.is_empty() {
            return None;
        }

        let mut headers: Vec<Vec<Inline>> = Vec::new();
        let mut body: Vec<Vec<Vec<Inline>>> = Vec::new();
        let mut span_warning = false;

        for tr in rows {
            let cells: Vec<ElementRef> = tr
                .children()
                .filter_map(ElementRef::wrap)
                .filter(|e| matches!(e.value().name(), "th" | "td"))
                .collect();
            if cells.is_empty() {
                continue;
            }
            if cells
                .iter()
                .any(|c| c.attr("colspan").is_some() || c.attr("rowspan").is_some())
            {
                span_warning = true;
            }
            let is_header = cells.iter().all(|c| c.value().name() == "th");
            let mut row = Vec::new();
            for cell in cells {
                row.push(self.collect_inline_element(cell, &Ctx::default()));
            }
            if row
                .iter()
                .all(|cell| cell.iter().all(|span| span.text.trim().is_empty()))
            {
                continue;
            }
            if headers.is_empty() && is_header {
                headers = row;
            } else {
                body.push(row);
            }
        }

        if span_warning {
            self.warnings
                .push("table spans were flattened for rendering".to_string());
        }

        let mut columns = headers.len();
        for row in &body {
            columns = columns.max(row.len());
        }
        if columns == 0 {
            return None;
        }
        headers.resize(columns, Vec::new());
        for row in &mut body {
            row.resize(columns, Vec::new());
        }

        Some(TableBlock {
            caption,
            headers,
            rows: body,
            aligns: vec![ColumnAlign::Left; columns],
        })
    }
}

fn selector(css: &str) -> Option<Selector> {
    Selector::parse(css).ok()
}

fn parse_dim(value: &str) -> Option<u32> {
    value
        .trim()
        .trim_end_matches("px")
        .parse::<f64>()
        .ok()
        .map(|v| v.max(0.0) as u32)
}

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "details"
            | "div"
            | "dl"
            | "dt"
            | "dd"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "tr"
            | "ul"
            | "img"
            | "video"
            | "audio"
            | "canvas"
            | "iframe"
            | "summary"
    )
}

fn is_skipped(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "style"
            | "noscript"
            | "template"
            | "iframe"
            | "form"
            | "button"
            | "input"
            | "select"
            | "textarea"
            | "svg"
            | "canvas"
            | "video"
            | "audio"
            | "nav"
            | "aside"
            | "footer"
            | "head"
            | "meta"
            | "link"
    )
}

fn is_hidden(element: &ElementRef<'_>) -> bool {
    if element.attr("hidden").is_some() || element.attr("aria-hidden") == Some("true") {
        return true;
    }
    let style = element.attr("style").unwrap_or("").to_ascii_lowercase();
    style.contains("display:none")
        || style.contains("display: none")
        || style.contains("visibility:hidden")
        || style.contains("visibility: hidden")
}

fn is_boilerplate(element: &ElementRef<'_>) -> bool {
    let class = element.attr("class").unwrap_or("").to_ascii_lowercase();
    let id = element.attr("id").unwrap_or("").to_ascii_lowercase();
    class
        .split_whitespace()
        .chain(id.split_whitespace())
        .any(|token| {
            token == "ad"
                || token.starts_with("ad-")
                || token == "ads"
                || token.contains("advert")
                || token.contains("promo")
                || token.contains("newsletter")
                || token.contains("subscribe")
                || token.contains("paywall")
                || token.contains("share")
                || token == "social"
                || token.contains("related")
                || token.contains("comments")
                || token.contains("cookie")
                || token.contains("consent")
                || token.contains("sponsored")
        })
}

fn should_skip(element: &ElementRef<'_>) -> bool {
    let name = element.value().name();
    is_skipped(name) || is_hidden(element) || is_boilerplate(element) || is_blank_anchor(element)
}

fn is_blank_anchor(element: &ElementRef<'_>) -> bool {
    if element.value().name() != "a" {
        return false;
    }
    let href = element.attr("href").unwrap_or("");
    let text = collapse_whitespace(&element.text().collect::<String>());
    href.is_empty() && text.is_empty()
}
