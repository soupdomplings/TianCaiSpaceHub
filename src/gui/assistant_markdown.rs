//! Selectable native Markdown blocks for the assistant's narrow transcript.
//! Content is text only: links and HTML never create browser or command actions.
use super::{assistant::scroll_transcript_from, theme};
use std::{cell::Cell, rc::Rc};
use wxdragon::{
    font::{Font, FontFamily, FontWeight},
    prelude::*,
    widgets::textctrl::{TextAttr, TextCtrl as MessageText},
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum TextKind {
    Paragraph,
    Heading(u8),
    List { marker: String, indent: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Block {
    Text(TextKind, String),
    Code {
        language: String,
        text: String,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Emphasis {
    Strong,
    Code,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
    emphasis: Emphasis,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Inline {
    text: String,
    spans: Vec<Span>,
}

fn inline(source: &str) -> Inline {
    let mut result = Inline::default();
    append_inline(source, &mut result);
    result
}

fn append_inline(source: &str, result: &mut Inline) {
    let mut index = 0;
    while index < source.len() {
        let rest = &source[index..];
        let ch = rest.chars().next().unwrap();
        if ch == '\\' {
            if let Some(next) = rest[1..].chars().next().filter(char::is_ascii_punctuation) {
                result.text.push(next);
                index += 1 + next.len_utf8();
                continue;
            }
        }
        if ch == '`' {
            let count = rest.bytes().take_while(|b| *b == b'`').count();
            let delimiter = &rest[..count];
            if let Some(close) = rest[count..].find(delimiter) {
                let content = rest[count..count + close].replace('\n', " ");
                let content = if content.starts_with(' ')
                    && content.ends_with(' ')
                    && !content.trim().is_empty()
                {
                    &content[1..content.len() - 1]
                } else {
                    &content
                };
                let start = result.text.len();
                result.text.push_str(content);
                result.spans.push(Span {
                    start,
                    end: result.text.len(),
                    emphasis: Emphasis::Code,
                });
                index += count + close + count;
                continue;
            }
        }
        if rest.starts_with("**") || rest.starts_with("__") {
            let delimiter = &rest[..2];
            if let Some(close) = rest[2..].find(delimiter).filter(|close| *close > 0) {
                let start = result.text.len();
                append_inline(&rest[2..2 + close], result);
                result.spans.push(Span {
                    start,
                    end: result.text.len(),
                    emphasis: Emphasis::Strong,
                });
                index += 2 + close + 2;
                continue;
            }
        }
        // Keep the destination copyable without creating clickable URLs.
        if ch == '[' {
            if let Some(label_end) = rest.find("](") {
                if let Some(url_end) = rest[label_end + 2..].find(')') {
                    append_inline(&rest[1..label_end], result);
                    result.text.push_str(" (");
                    result
                        .text
                        .push_str(&rest[label_end + 2..label_end + 2 + url_end]);
                    result.text.push(')');
                    index += label_end + 2 + url_end + 1;
                    continue;
                }
            }
        }
        result.text.push(ch);
        index += ch.len_utf8();
    }
}

fn fence(line: &str) -> Option<(char, usize, &str)> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let marker = trimmed.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let count = trimmed.chars().take_while(|c| *c == marker).count();
    (count >= 3).then(|| (marker, count, trimmed[count..].trim()))
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let line = line.trim_start();
    let count = line.bytes().take_while(|b| *b == b'#').count();
    if !(1..=6).contains(&count)
        || !line[count..].starts_with(char::is_whitespace) && line.len() != count
    {
        return None;
    }
    let text = line[count..].trim();
    let without_hashes = text.trim_end_matches('#');
    let text = if without_hashes.ends_with(char::is_whitespace) {
        without_hashes.trim_end()
    } else {
        text
    };
    Some((count as u8, text))
}

fn list_item(line: &str) -> Option<(TextKind, &str)> {
    let rest = line.trim_start();
    let indent = line[..line.len() - rest.len()]
        .chars()
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum::<usize>()
        / 2;
    if matches!(rest.as_bytes().first(), Some(b'-' | b'*' | b'+'))
        && rest[1..].starts_with(char::is_whitespace)
    {
        return Some((
            TextKind::List {
                marker: "•".into(),
                indent,
            },
            rest[1..].trim_start(),
        ));
    }
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0
        && digits <= 9
        && matches!(rest.as_bytes().get(digits), Some(b'.' | b')'))
        && rest[digits + 1..].starts_with(char::is_whitespace)
    {
        return Some((
            TextKind::List {
                marker: format!("{}.", &rest[..digits]),
                indent,
            },
            rest[digits + 1..].trim_start(),
        ));
    }
    None
}

/// Pipe splitting respects escaped pipes and inline-code spans.
fn table_cells(line: &str) -> Option<Vec<String>> {
    let line = line.trim();
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    let mut code_ticks = 0;
    let mut chars = line.chars().peekable();
    let mut has_pipe = false;
    while let Some(ch) = chars.next() {
        if escaped {
            cell.push(ch);
            escaped = false;
        } else if ch == '\\' {
            cell.push(ch);
            escaped = true;
        } else if ch == '`' {
            let mut count = 1;
            cell.push(ch);
            while chars.peek() == Some(&'`') {
                cell.push(chars.next().unwrap());
                count += 1;
            }
            if code_ticks == 0 {
                code_ticks = count;
            } else if code_ticks == count {
                code_ticks = 0;
            }
        } else if ch == '|' && code_ticks == 0 {
            cells.push(cell.trim().to_owned());
            cell.clear();
            has_pipe = true;
        } else {
            cell.push(ch);
        }
    }
    cells.push(cell.trim().to_owned());
    if line.starts_with('|') && cells.first().is_some_and(String::is_empty) {
        cells.remove(0);
    }
    if line.ends_with('|') && cells.last().is_some_and(String::is_empty) {
        cells.pop();
    }
    (has_pipe && !cells.is_empty()).then_some(cells)
}

fn table_header(lines: &[&str], index: usize) -> Option<Vec<String>> {
    let headers = table_cells(lines[index])?;
    let separator = table_cells(lines.get(index + 1)?)?;
    (separator.len() == headers.len()
        && separator.iter().all(|cell| {
            let dashes = cell.trim_matches(':');
            dashes.len() >= 3 && dashes.bytes().all(|b| b == b'-')
        }))
    .then_some(headers)
}

fn parse(source: &str) -> Vec<Block> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].trim().is_empty() {
            index += 1;
            continue;
        }
        if let Some((marker, count, language)) = fence(lines[index]) {
            let language = language.to_owned();
            index += 1;
            let first = index;
            while index < lines.len()
                && !fence(lines[index])
                    .is_some_and(|(m, n, tail)| m == marker && n >= count && tail.is_empty())
            {
                index += 1;
            }
            let text = lines[first..index].join("\n");
            if index < lines.len() {
                index += 1;
            }
            blocks.push(Block::Code { language, text });
            continue;
        }
        if let Some((level, text)) = heading(lines[index]) {
            blocks.push(Block::Text(TextKind::Heading(level), text.into()));
            index += 1;
            continue;
        }
        if let Some(mut headers) = table_header(&lines, index) {
            index += 2;
            let mut rows = Vec::new();
            while index < lines.len() {
                let Some(row) = table_cells(lines[index]) else {
                    break;
                };
                rows.push(row);
                index += 1;
            }
            let columns = rows
                .iter()
                .map(Vec::len)
                .max()
                .unwrap_or(0)
                .max(headers.len());
            headers.resize(columns, String::new());
            for row in &mut rows {
                row.resize(columns, String::new());
            }
            blocks.push(Block::Table { headers, rows });
            continue;
        }
        if let Some((kind, text)) = list_item(lines[index]) {
            let mut text = text.to_owned();
            index += 1;
            while index < lines.len()
                && !lines[index].trim().is_empty()
                && lines[index].starts_with(char::is_whitespace)
                && list_item(lines[index]).is_none()
                && fence(lines[index]).is_none()
            {
                text.push('\n');
                text.push_str(lines[index].trim());
                index += 1;
            }
            blocks.push(Block::Text(kind, text));
            continue;
        }
        let first = index;
        index += 1;
        while index < lines.len()
            && !lines[index].trim().is_empty()
            && fence(lines[index]).is_none()
            && heading(lines[index]).is_none()
            && list_item(lines[index]).is_none()
            && table_header(&lines, index).is_none()
        {
            index += 1;
        }
        blocks.push(Block::Text(
            TextKind::Paragraph,
            lines[first..index].join("\n"),
        ));
    }
    if blocks.is_empty() {
        blocks.push(Block::Text(TextKind::Paragraph, String::new()));
    }
    blocks
}

#[derive(Clone, Copy)]
enum FontKind {
    Body,
    Heading(u8),
    Code,
    Header,
}

fn font(kind: FontKind) -> Font {
    if matches!(kind, FontKind::Code) {
        let builder = Font::builder()
            .with_family(FontFamily::Teletype)
            .with_point_size(9);
        #[cfg(windows)]
        let builder = builder.with_face_name("Consolas");
        #[cfg(target_os = "macos")]
        let builder = builder.with_face_name("Menlo");
        if let Some(font) = builder.build() {
            return font;
        }
    }
    let mut font = theme::font(theme::TextRole::Body);
    if let FontKind::Heading(level) = kind {
        font.set_point_size(match level {
            1 => 14,
            2 => 12,
            _ => 11,
        });
    }
    if matches!(kind, FontKind::Heading(_) | FontKind::Header) {
        font.set_weight(FontWeight::Bold);
    }
    font
}

fn tint(background: Colour, foreground: Colour) -> Colour {
    let mix = |a: u8, b: u8| ((u16::from(a) * 15 + u16::from(b)) / 16) as u8;
    Colour::rgb(
        mix(background.r, foreground.r),
        mix(background.g, foreground.g),
        mix(background.b, foreground.b),
    )
}

// wxMSW's Rich2 positions count UTF-16 code units; other ports use characters.
fn native_offset(text: &str) -> i64 {
    #[cfg(windows)]
    {
        text.encode_utf16().count() as i64
    }
    #[cfg(not(windows))]
    {
        text.chars().count() as i64
    }
}

struct TextLeaf {
    editor: MessageText,
    value: Inline,
    kind: FontKind,
    foreground: Colour,
    background: Colour,
    width: Cell<i32>,
    height: Cell<i32>,
}

impl TextLeaf {
    fn new(
        parent: &Panel,
        value: Inline,
        kind: FontKind,
        foreground: Colour,
        background: Colour,
        transcript: ScrolledWindow,
        escape: Option<&Rc<dyn Fn()>>,
    ) -> Self {
        let editor = MessageText::builder(parent)
            .with_style(
                TextCtrlStyle::MultiLine
                    | TextCtrlStyle::ReadOnly
                    | TextCtrlStyle::Rich2
                    | TextCtrlStyle::BorderNone
                    | TextCtrlStyle::NoVScroll
                    | TextCtrlStyle::CharWrap
                    | TextCtrlStyle::NoHideSel,
            )
            .build();
        editor.set_name("回复内容");
        editor.set_background_color(background);
        editor.set_foreground_color(foreground);
        editor.set_font(&font(kind));
        scroll_transcript_from(&editor, transcript);
        let mut leaf = Self {
            editor,
            value: Inline::default(),
            kind,
            foreground,
            background,
            width: Cell::new(-1),
            height: Cell::new(0),
        };
        leaf.update(value);
        if let Some(escape) = escape {
            leaf.bind_escape(escape.clone());
        }
        leaf
    }

    fn bind_escape(&self, callback: Rc<dyn Fn()>) {
        self.editor.on_key_down(move |event| {
            if matches!(&event, WindowEventData::Keyboard(key) if key.get_key_code() == Some(27)) {
                callback();
                event.skip(false);
            } else {
                event.skip(true);
            }
        });
    }

    fn update(&mut self, value: Inline) {
        if self.value == value && self.height.get() != 0 {
            return;
        }
        let selection = self.editor.get_selection();
        if let Some(suffix) = value.text.strip_prefix(&self.value.text) {
            if !suffix.is_empty() {
                self.editor.append_text(suffix);
            }
        } else {
            self.editor.change_value(&value.text);
        }
        let mut base = TextAttr::new();
        base.set_font(&font(self.kind));
        base.set_text_colour(self.foreground);
        base.set_background_colour(self.background);
        self.editor.set_default_style(&base);
        self.editor
            .set_style(0, self.editor.get_last_position(), &base);
        // Strong and code are independent attributes, so nested spans combine.
        for span in &value.spans {
            let mut style = TextAttr::new();
            match span.emphasis {
                Emphasis::Strong => style.set_font_weight(FontWeight::Bold),
                Emphasis::Code => {
                    style.set_font(&font(FontKind::Code));
                    style.set_background_colour(tint(self.background, self.foreground));
                }
            }
            self.editor.set_style(
                native_offset(&value.text[..span.start]),
                native_offset(&value.text[..span.end]),
                &style,
            );
        }
        let last = self.editor.get_last_position();
        self.editor
            .set_selection(selection.0.min(last), selection.1.min(last));
        self.value = value;
        self.width.set(-1);
    }

    fn layout(&self, width: i32) -> i32 {
        let width = width.max(1);
        if self.width.replace(width) == width {
            return self.height.get();
        }
        self.editor
            .set_size(Size::new(width, self.editor.get_char_height().max(1)));
        let usable = (width - self.editor.from_dip(6)).max(1);
        let measured = self
            .value
            .text
            .split('\n')
            .map(|line| {
                let extent = self.editor.get_text_extent(line).width;
                (extent + usable - 1) / usable
            })
            .map(|lines| lines.max(1))
            .sum::<i32>();
        let lines = self.editor.get_number_of_lines().max(measured).max(1);
        let height = lines * self.editor.get_char_height().max(1) + self.editor.from_dip(8);
        self.editor.set_min_size(Size::new(width, height));
        self.editor.set_size(Size::new(width, height));
        self.height.set(height);
        height
    }
}

struct TableCell {
    panel: Panel,
    key: StaticText,
    value: TextLeaf,
}

struct TableView {
    headers: Vec<TextLeaf>,
    cells: Vec<Vec<TableCell>>,
}

impl TableView {
    fn update(
        &mut self,
        panel: &Panel,
        headers: &[String],
        rows: &[Vec<String>],
        foreground: Colour,
        background: Colour,
        transcript: ScrolledWindow,
        escape: Option<&Rc<dyn Fn()>>,
    ) {
        while self.headers.len() > headers.len() {
            self.headers.pop().unwrap().editor.destroy();
        }
        for (index, header) in headers.iter().enumerate() {
            if let Some(leaf) = self.headers.get_mut(index) {
                leaf.update(inline(header));
            } else {
                self.headers.push(TextLeaf::new(
                    panel,
                    inline(header),
                    FontKind::Header,
                    foreground,
                    tint(background, foreground),
                    transcript,
                    escape,
                ));
            }
        }
        while self.cells.len() > rows.len() {
            for cell in self.cells.pop().unwrap() {
                cell.panel.destroy();
            }
        }
        for (row_index, row) in rows.iter().enumerate() {
            if self.cells.len() <= row_index {
                self.cells.push(Vec::new());
            }
            let cells = &mut self.cells[row_index];
            while cells.len() > row.len() {
                cells.pop().unwrap().panel.destroy();
            }
            for (column, source) in row.iter().enumerate() {
                let heading = inline(&headers[column]).text;
                if let Some(cell) = cells.get_mut(column) {
                    cell.key.set_label(&heading);
                    cell.value.update(inline(source));
                } else {
                    let host = Panel::builder(panel).build();
                    host.set_background_color(background);
                    scroll_transcript_from(&host, transcript);
                    let key = StaticText::builder(&host).with_label(&heading).build();
                    key.set_font(&font(FontKind::Header));
                    key.set_foreground_color(foreground);
                    key.set_background_color(background);
                    scroll_transcript_from(&key, transcript);
                    let value = TextLeaf::new(
                        &host,
                        inline(source),
                        FontKind::Body,
                        foreground,
                        background,
                        transcript,
                        escape,
                    );
                    cells.push(TableCell {
                        panel: host,
                        key,
                        value,
                    });
                }
            }
        }
    }

    fn layout(&self, panel: &Panel, width: i32) -> i32 {
        let columns = self.headers.len().max(1);
        let gap = panel.from_dip(1);
        let padding = panel.from_dip(6);
        let stacked = !self.cells.is_empty() && width / (columns as i32) < panel.from_dip(76);
        let mut y = gap;
        if stacked {
            for header in &self.headers {
                header.editor.hide();
            }
            // A narrow table becomes records. Labels and values still align,
            // and no horizontal scrolling or hidden columns are introduced.
            for row in &self.cells {
                for cell in row {
                    cell.key.show(true);
                    let key_width = (width / 3).max(1);
                    let value_width = (width - key_width - padding * 3).max(1);
                    cell.key.wrap((key_width - padding).max(1));
                    let height = cell
                        .value
                        .layout(value_width)
                        .max(cell.key.get_best_size().height)
                        + padding * 2;
                    cell.panel
                        .set_size_with_pos(gap, y, (width - gap * 2).max(1), height);
                    cell.key.set_size_with_pos(
                        padding,
                        padding,
                        key_width,
                        (height - padding * 2).max(1),
                    );
                    cell.value.editor.set_size_with_pos(
                        key_width + padding * 2,
                        padding,
                        value_width,
                        height - padding * 2,
                    );
                    y += height;
                }
                y += panel.from_dip(5);
            }
        } else {
            let available = (width - gap * (columns as i32 + 1)).max(columns as i32);
            let base = available / columns as i32;
            let widths = (0..columns)
                .map(|column| base + i32::from((column as i32) < available % columns as i32))
                .collect::<Vec<_>>();
            let header_height = self
                .headers
                .iter()
                .zip(&widths)
                .map(|(h, width)| h.layout((width - padding * 2).max(1)))
                .max()
                .unwrap_or(0)
                + padding * 2;
            let mut x = gap;
            for (header, width) in self.headers.iter().zip(&widths) {
                header.editor.show(true);
                header.editor.set_size_with_pos(
                    x + padding,
                    y + padding,
                    (width - padding * 2).max(1),
                    header_height - padding * 2,
                );
                x += width + gap;
            }
            y += header_height + gap;
            for row in &self.cells {
                let height = row
                    .iter()
                    .zip(&widths)
                    .map(|(cell, width)| cell.value.layout((width - padding * 2).max(1)))
                    .max()
                    .unwrap_or(0)
                    + padding * 2;
                let mut x = gap;
                for (cell, width) in row.iter().zip(&widths) {
                    cell.key.hide();
                    cell.panel.set_size_with_pos(x, y, *width, height);
                    cell.value.editor.set_size_with_pos(
                        padding,
                        padding,
                        (width - padding * 2).max(1),
                        height - padding * 2,
                    );
                    x += width + gap;
                }
                y += height + gap;
            }
        }
        y
    }
}

enum Content {
    Text(TextLeaf),
    Code { caption: StaticText, text: TextLeaf },
    Table(TableView),
}

struct BlockView {
    model: Block,
    panel: Panel,
    content: Content,
    width: Cell<i32>,
    height: Cell<i32>,
}

fn text_value(kind: &TextKind, source: &str) -> Inline {
    let mut value = Inline::default();
    if let TextKind::List { marker, indent } = kind {
        value.text = format!("{}{} ", "  ".repeat((*indent).min(8)), marker);
    }
    append_inline(source, &mut value);
    value
}

impl BlockView {
    fn new(
        parent: &Panel,
        model: Block,
        foreground: Colour,
        background: Colour,
        transcript: ScrolledWindow,
        escape: Option<&Rc<dyn Fn()>>,
    ) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_color(background);
        scroll_transcript_from(&panel, transcript);
        let content = match &model {
            Block::Text(kind, source) => Content::Text(TextLeaf::new(
                &panel,
                text_value(kind, source),
                match kind {
                    TextKind::Heading(level) => FontKind::Heading(*level),
                    _ => FontKind::Body,
                },
                foreground,
                background,
                transcript,
                escape,
            )),
            Block::Code { language, text } => {
                let t = theme::theme();
                panel.set_background_color(t.code_bg);
                let caption = StaticText::builder(&panel).with_label(language).build();
                caption.set_font(&theme::font(theme::TextRole::Caption));
                caption.set_foreground_color(t.ink_secondary);
                caption.set_background_color(t.code_bg);
                scroll_transcript_from(&caption, transcript);
                let text = TextLeaf::new(
                    &panel,
                    Inline {
                        text: text.clone(),
                        spans: vec![],
                    },
                    FontKind::Code,
                    t.code_fg,
                    t.code_bg,
                    transcript,
                    escape,
                );
                Content::Code { caption, text }
            }
            Block::Table { headers, rows } => {
                panel.set_background_color(tint(background, foreground));
                let mut table = TableView {
                    headers: vec![],
                    cells: vec![],
                };
                table.update(
                    &panel, headers, rows, foreground, background, transcript, escape,
                );
                Content::Table(table)
            }
        };
        Self {
            model,
            panel,
            content,
            width: Cell::new(-1),
            height: Cell::new(0),
        }
    }

    fn compatible(&self, next: &Block) -> bool {
        match (&self.model, next) {
            (Block::Text(a, _), Block::Text(b, _)) => a == b,
            (Block::Code { .. }, Block::Code { .. })
            | (Block::Table { .. }, Block::Table { .. }) => true,
            _ => false,
        }
    }

    fn update(
        &mut self,
        model: Block,
        foreground: Colour,
        background: Colour,
        transcript: ScrolledWindow,
        escape: Option<&Rc<dyn Fn()>>,
    ) {
        match (&mut self.content, &model) {
            (Content::Text(leaf), Block::Text(kind, text)) => leaf.update(text_value(kind, text)),
            (
                Content::Code {
                    caption,
                    text: leaf,
                },
                Block::Code { language, text },
            ) => {
                caption.set_label(language);
                leaf.update(Inline {
                    text: text.clone(),
                    spans: vec![],
                });
            }
            (Content::Table(table), Block::Table { headers, rows }) => table.update(
                &self.panel,
                headers,
                rows,
                foreground,
                background,
                transcript,
                escape,
            ),
            _ => unreachable!("only compatible Markdown blocks are updated"),
        }
        self.model = model;
        self.width.set(-1);
    }

    fn controls(&self) -> Vec<MessageText> {
        match &self.content {
            Content::Text(leaf) | Content::Code { text: leaf, .. } => vec![leaf.editor],
            Content::Table(table) => table
                .headers
                .iter()
                .map(|leaf| leaf.editor)
                .chain(table.cells.iter().flatten().map(|cell| cell.value.editor))
                .collect(),
        }
    }

    fn layout(&self, width: i32) -> i32 {
        if self.width.replace(width) == width {
            return self.height.get();
        }
        let height = match &self.content {
            Content::Text(leaf) => {
                let height = leaf.layout(width);
                leaf.editor.set_size_with_pos(0, 0, width, height);
                height
            }
            Content::Code { caption, text } => {
                let padding = self.panel.from_dip(8);
                let inner = (width - padding * 2).max(1);
                let caption_height = if caption.get_label().is_empty() {
                    caption.hide();
                    0
                } else {
                    caption.show(true);
                    caption.wrap(inner);
                    let height = caption.get_best_size().height + self.panel.from_dip(4);
                    caption.set_size_with_pos(padding, padding, inner, height);
                    height
                };
                let text_height = text.layout(inner);
                text.editor.set_size_with_pos(
                    padding,
                    padding + caption_height,
                    inner,
                    text_height,
                );
                text_height + caption_height + padding * 2
            }
            Content::Table(table) => table.layout(&self.panel, width),
        };
        self.panel.set_min_size(Size::new(width, height));
        self.height.set(height);
        height
    }
}

pub(super) struct MessageBody {
    pub panel: Panel,
    blocks: Vec<BlockView>,
    source: String,
    foreground: Colour,
    background: Colour,
    transcript: ScrolledWindow,
    escape: Option<Rc<dyn Fn()>>,
}

impl MessageBody {
    pub(super) fn new(
        parent: &Panel,
        text: &str,
        foreground: Colour,
        background: Colour,
        transcript: ScrolledWindow,
    ) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_color(background);
        scroll_transcript_from(&panel, transcript);
        let blocks = parse(text)
            .into_iter()
            .map(|block| BlockView::new(&panel, block, foreground, background, transcript, None))
            .collect();
        Self {
            panel,
            blocks,
            source: text.into(),
            foreground,
            background,
            transcript,
            escape: None,
        }
    }

    pub(super) fn on_escape(&mut self, callback: Rc<dyn Fn()>) {
        for block in &self.blocks {
            for editor in block.controls() {
                let callback = callback.clone();
                editor.on_key_down(move |event| {
                    if matches!(&event, WindowEventData::Keyboard(key) if key.get_key_code() == Some(27)) { callback(); event.skip(false); } else { event.skip(true); }
                });
            }
        }
        self.escape = Some(callback);
    }

    pub(super) fn update(&mut self, text: &str) {
        if self.source == text {
            return;
        }
        let next = parse(text);
        // Streaming appends leave completed blocks and their selections intact.
        // Only a changed compatible block is edited; structural changes replace
        // the remaining tail, whose previous interpretation is no longer valid.
        let mut index = 0;
        while index < next.len() && index < self.blocks.len() {
            if self.blocks[index].model == next[index] {
                index += 1;
                continue;
            }
            if !self.blocks[index].compatible(&next[index]) {
                break;
            }
            self.blocks[index].update(
                next[index].clone(),
                self.foreground,
                self.background,
                self.transcript,
                self.escape.as_ref(),
            );
            index += 1;
        }
        while self.blocks.len() > index {
            self.blocks.pop().unwrap().panel.destroy();
        }
        for block in next.into_iter().skip(index) {
            self.blocks.push(BlockView::new(
                &self.panel,
                block,
                self.foreground,
                self.background,
                self.transcript,
                self.escape.as_ref(),
            ));
        }
        self.source = text.into();
    }

    /// Width and returned height are physical pixels, matching MessageCard.
    pub(super) fn layout(&self, width: i32) -> i32 {
        let width = width.max(1);
        let mut y = 0;
        for (index, block) in self.blocks.iter().enumerate() {
            if index > 0 {
                y += self.panel.from_dip(
                    if matches!(&block.model, Block::Text(TextKind::List { .. }, _)) {
                        3
                    } else {
                        8
                    },
                );
            }
            let height = block.layout(width);
            block.panel.set_size_with_pos(0, y, width, height);
            y += height;
        }
        self.panel.set_min_size(Size::new(width, y));
        self.panel.set_size(Size::new(width, y));
        y
    }

    pub(super) fn primary_text(&self) -> MessageText {
        self.blocks[0].controls()[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_blocks_preserve_code_lists_headings_and_incomplete_streams() {
        let source = "# 标题\n\n普通 **重点** 与 `model_id`。\n\n1. 第一步\n2. 第二步\n\n```json\n{\"model\":\"gpt-6.2\"}\n```\n\n末段";
        let blocks = parse(source);
        assert_eq!(blocks.len(), 6);
        assert_eq!(blocks[0], Block::Text(TextKind::Heading(1), "标题".into()));
        assert!(
            matches!(&blocks[2], Block::Text(TextKind::List { marker, .. }, _) if marker == "1.")
        );
        assert_eq!(
            blocks[4],
            Block::Code {
                language: "json".into(),
                text: "{\"model\":\"gpt-6.2\"}".into()
            }
        );
        let extended = parse(&format!("{source}继续"));
        assert_eq!(&blocks[..5], &extended[..5]);
        assert_eq!(
            parse("```rust\nlet value = 1;"),
            vec![Block::Code {
                language: "rust".into(),
                text: "let value = 1;".into()
            }]
        );
        assert_eq!(inline("未完成 **重点").text, "未完成 **重点");
    }

    #[test]
    fn markdown_table_keeps_escaped_pipes_inline_code_and_extra_cells() {
        let blocks =
            parse("| 字段 | 值 |\n| :--- | ---: |\n| `a|b` | x\\|y |\n| 另一行 | 值 | 额外内容 |");
        let Block::Table { headers, rows } = &blocks[0] else {
            panic!("expected a native table")
        };
        assert_eq!(headers, &["字段", "值", ""]);
        assert_eq!(rows.len(), 2);
        assert_eq!(inline(&rows[0][0]).text, "a|b");
        assert_eq!(inline(&rows[0][1]).text, "x|y");
        assert_eq!(rows[1][2], "额外内容");
        assert!(!matches!(
            parse("a | b\n还未完成分隔行")[0],
            Block::Table { .. }
        ));
    }

    #[test]
    fn markdown_inline_styles_keep_unicode_and_destinations_as_plain_text() {
        let value =
            inline("中文 **重点 `模型`** [帮助](https://example.com/guide) <script>plain</script>");
        assert_eq!(
            value.text,
            "中文 重点 模型 帮助 (https://example.com/guide) <script>plain</script>"
        );
        assert!(
            value
                .spans
                .iter()
                .any(|span| span.emphasis == Emphasis::Code
                    && &value.text[span.start..span.end] == "模型")
        );
        assert!(
            value
                .spans
                .iter()
                .any(|span| span.emphasis == Emphasis::Strong
                    && &value.text[span.start..span.end] == "重点 模型")
        );
        assert_eq!(inline("\\*literal\\* 与 ``a`b``").text, "*literal* 与 a`b");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Explicit isolated wxWidgets rendering; run with --test-threads=1"]
    fn native_markdown_reflows_tables_and_keeps_streamed_prefix_selection() {
        wxdragon::app::render_offscreen(|| {
            theme::init(theme::ThemeMode::Light);
            let frame = Frame::builder()
                .with_title("synthetic Markdown transcript")
                .with_size(Size::new(800, 800))
                .build();
            let transcript = ScrolledWindow::builder(&frame)
                .with_style(ScrolledWindowStyle::VScroll)
                .build();
            let host = Panel::builder(&transcript).build();
            let source = format!(
                "# 标题\n\n段落 **重点** 与 `gpt-6.2`，保留原生选择复制。\n\n1. 读取配置\n2. 保存后回读\n\n```text\n{}\n```\n\n| 模型 | 参数 | 状态 |\n| --- | --- | --- |\n| {} | **原生单元格** | 已配置 |",
                "long_code_or_url_without_breaks_".repeat(35),
                "https://example.com/very-long-model-name-without-spaces".repeat(8),
            );
            let t = theme::theme();
            let mut body = MessageBody::new(&host, &source, t.ink_primary, t.bg_card, transcript);
            let first = body.primary_text();
            first.set_selection(0, 2);
            let wide = host.from_dip(610);
            let wide_height = body.layout(wide);
            let table_index = body.blocks.iter().position(|block| matches!(block.content, Content::Table(_))).unwrap();
            let Content::Table(table) = &body.blocks[table_index].content else { unreachable!() };
            let first_cell = table.cells[0][0].value.editor;
            first_cell.set_selection(0, 5);
            assert!(table.headers.iter().all(|h| h.editor.is_shown()));
            let narrow = host.from_dip(210);
            assert!(body.layout(narrow) > wide_height);
            assert!(table.headers.iter().all(|h| !h.editor.is_shown()));
            for row in &table.cells {
                for cell in row {
                    let position = cell.value.editor.get_position();
                    let size = cell.value.editor.get_size();
                    assert!(position.x >= 0 && position.x + size.width <= cell.panel.get_size().width);
                    assert!(cell.key.is_shown());
                }
            }
            body.update(&format!("{source}\n| 第二个模型 | `contextWindow` | 新增 |"));
            body.layout(narrow);
            assert_eq!(body.primary_text().handle_ptr(), first.handle_ptr());
            assert_eq!(first.get_string_selection(), "标题");
            let Content::Table(table) = &body.blocks[table_index].content else { unreachable!() };
            assert_eq!(table.cells[0][0].value.editor.handle_ptr(), first_cell.handle_ptr());
            assert_eq!(first_cell.get_selection(), (0, 5));
            assert_eq!(table.cells.len(), 2);
            body.layout(wide);
            assert!(table.headers.iter().all(|h| h.editor.is_shown()));
            assert!(table.cells.iter().flatten().all(|cell| !cell.key.is_shown()));
            frame.destroy();
        })
        .unwrap();
    }
}
