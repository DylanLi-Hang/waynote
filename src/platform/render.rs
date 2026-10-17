//! Markdown preview renderer shared by note cards.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::gio;
use gtk::pango::{Style, Underline};
use gtk::prelude::*;
use gtk::{
    CheckButton, CssProvider, GestureClick, Label,
    Orientation, Picture, PolicyType, ScrolledWindow, TextTag, TextView, WrapMode,
};

use crate::core::markdown::{
    self, Block, Inline, List, ListItem, RenderDoc, Table,
    classify_image, heading_style, is_web_link, ImageSrc,
};
use crate::core::theme::{self, Theme};

// ── CSS installation ──────────────────────────────────────────────────────────

/// Install the card CSS for all seven colour variants (idempotent per display).
pub fn install_card_css() {
    let css = CssProvider::new();
    #[allow(deprecated)]
    css.load_from_data(CARD_CSS);
    #[allow(deprecated)]
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("display"),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

const CARD_CSS: &str = "
.waynote-table-cell { padding: 5px 6px; border-bottom: 1px solid rgba(0,0,0,0.14); }
.waynote-card {
    border-radius: 10px;
    padding: 14px 16px;
    box-shadow: 0 3px 12px rgba(0,0,0,0.35);
}
.waynote-card.yellow {
    background: #F6EBA8;
    border: 1px solid #E3D58C;
}
.waynote-card.green {
    background: #CDE8C5;
    border: 1px solid #B8D9AE;
}
.waynote-card.blue {
    background: #C7DDF1;
    border: 1px solid #AECBE6;
}
.waynote-card.pink {
    background: #F4D2DA;
    border: 1px solid #E6BAC5;
}
.waynote-card.purple {
    background: #E0D4F0;
    border: 1px solid #CBBCE6;
}
.waynote-card.gray {
    background: #DEDCD6;
    border: 1px solid #C9C7C0;
}
.waynote-card.orange {
    background: #F6D9B8;
    border: 1px solid #E8C49A;
}
.waynote-card.waynote-active { border-color: #3A86FF; }
/* Markdown task checkboxes (GTK4 tree: checkbutton > check). Replace Adwaita's
   heavy dark indicator with a light outline box in the paper's ink — muted when
   empty, the paper accent when checked — so done/undone read clearly on pastel. */
.waynote-card checkbutton {
    min-width: 16px;
    min-height: 16px;
    padding: 0 6px 0 0;
    margin: 0;
    background: transparent;
    background-image: none;
    border: none;
    box-shadow: none;
}
.waynote-card checkbutton > check {
    min-width: 13px;
    min-height: 13px;
    margin: 0;
    border-radius: 4px;
    border: 1px solid alpha(currentColor, 0.48);
    background: transparent;
    box-shadow: none;
    color: currentColor;
}
.waynote-card checkbutton:hover > check {
    border-color: alpha(currentColor, 0.72);
    background-color: alpha(currentColor, 0.06);
}
.waynote-card checkbutton > check:checked {
    border-color: currentColor;
    background-color: alpha(currentColor, 0.14);
}
.waynote-card.yellow checkbutton { color: #8A7F5E; }
.waynote-card.green  checkbutton { color: #5E7355; }
.waynote-card.blue   checkbutton { color: #5A6E80; }
.waynote-card.pink   checkbutton { color: #836670; }
.waynote-card.purple checkbutton { color: #6E6383; }
.waynote-card.gray   checkbutton { color: #73726C; }
.waynote-card.orange checkbutton { color: #8A7152; }
.waynote-card.yellow checkbutton > check:checked { color: #B07A1E; }
.waynote-card.green  checkbutton > check:checked { color: #3E7A3E; }
.waynote-card.blue   checkbutton > check:checked { color: #2C6FA8; }
.waynote-card.pink   checkbutton > check:checked { color: #B0506E; }
.waynote-card.purple checkbutton > check:checked { color: #6B4FA0; }
.waynote-card.gray   checkbutton > check:checked { color: #5E5C55; }
.waynote-card.orange checkbutton > check:checked { color: #BD6415; }
/* Transparent so the paper bg shows AND so widget CSS never sets a text colour.
   All ink/accent/muted colour is applied programmatically via TextTags — Adwaita
   widget CSS would otherwise override TextTag foregrounds (PoC lesson). Do NOT
   give these a concrete colour here. */
.waynote-card textview,
.waynote-card text {
    background: transparent;
    color: transparent;
}
.waynote-header { border-bottom: 1px solid rgba(0,0,0,0.10); }
.waynote-header-title { color: #403A28; font-weight: bold; }
/* Edit-mode raw-markdown view: needs an explicit readable colour because the
   `.waynote-card text { color: transparent }` rule above would otherwise hide it.
   Two-class specificity beats that rule. Monospace + a visible caret. */
.waynote-card .waynote-edit text,
.waynote-card .waynote-edit {
    color: #2e2a22;
    caret-color: #2e2a22;
    font-family: monospace;
}
";

// ── TagStore — lazy-built TextTags ───────────────────────────────────────────

struct TagStore {
    buffer: gtk::TextBuffer,
    theme: Theme,
    heading: [Option<TextTag>; 6],
    bold: Option<TextTag>,
    italic: Option<TextTag>,
    strike: Option<TextTag>,
    code_inline: Option<TextTag>,
    code_block: Option<TextTag>,
    code_lang: Option<TextTag>,
    blockquote: [Option<TextTag>; 4],
    para_spacing: Option<TextTag>,
    hr: Option<TextTag>,
    body: Option<TextTag>,
    muted: Option<TextTag>,
    accent: Option<TextTag>,
    h1_underline: Option<TextTag>,
    /// Left-margin tags keyed by indent px, so re-renders reuse them instead of
    /// accumulating anonymous tags in the buffer's tag table.
    margin: HashMap<i32, TextTag>,
}

impl TagStore {
    fn new(buffer: gtk::TextBuffer, theme: Theme) -> Self {
        Self {
            buffer,
            theme,
            heading: Default::default(),
            bold: None,
            italic: None,
            strike: None,
            code_inline: None,
            code_block: None,
            code_lang: None,
            blockquote: Default::default(),
            para_spacing: None,
            hr: None,
            body: None,
            muted: None,
            accent: None,
            h1_underline: None,
            margin: HashMap::new(),
        }
    }

    /// A left-margin tag for `indent` px, cached so repeated list items at the
    /// same depth reuse one tag instead of leaking anonymous tags.
    fn margin_tag(&mut self, indent: i32) -> TextTag {
        if let Some(tag) = self.margin.get(&indent) {
            return tag.clone();
        }
        let tag = TextTag::new(Some(&format!("margin-{indent}")));
        tag.set_left_margin(indent);
        self.buffer.tag_table().add(&tag);
        self.margin.insert(indent, tag.clone());
        tag
    }

    fn body_tag(&mut self) -> &TextTag {
        if self.body.is_none() {
            let tag = TextTag::new(Some("body"));
            tag.set_foreground(Some(self.theme.ink));
            self.buffer.tag_table().add(&tag);
            self.body = Some(tag);
        }
        self.body.as_ref().unwrap()
    }

    fn muted_tag(&mut self) -> &TextTag {
        if self.muted.is_none() {
            let tag = TextTag::new(Some("muted"));
            tag.set_foreground(Some(self.theme.muted));
            self.buffer.tag_table().add(&tag);
            self.muted = Some(tag);
        }
        self.muted.as_ref().unwrap()
    }

    fn accent_tag(&mut self) -> &TextTag {
        if self.accent.is_none() {
            let tag = TextTag::new(Some("accent"));
            tag.set_foreground(Some(self.theme.accent));
            self.buffer.tag_table().add(&tag);
            self.accent = Some(tag);
        }
        self.accent.as_ref().unwrap()
    }

    fn h1_underline_tag(&mut self) -> &TextTag {
        if self.h1_underline.is_none() {
            let tag = TextTag::new(Some("h1-underline"));
            tag.set_foreground(Some(self.theme.accent));
            tag.set_underline(Underline::Single);
            self.buffer.tag_table().add(&tag);
            self.h1_underline = Some(tag);
        }
        self.h1_underline.as_ref().unwrap()
    }

    fn heading_tag(&mut self, level: u8) -> &TextTag {
        let idx = (level.clamp(1, 6) - 1) as usize;
        if self.heading[idx].is_none() {
            let style = heading_style(level);
            let name = format!("heading-{level}");
            let tag = TextTag::new(Some(&name));
            tag.set_scale(style.scale);
            tag.set_weight(style.weight as i32);
            if style.italic {
                tag.set_style(Style::Italic);
            }
            // h5/h6 are near body size, so they also take the accent colour to stay
            // unmistakably "heading"; larger levels use ink.
            let fg = if style.accent { self.theme.accent } else { self.theme.ink };
            tag.set_foreground(Some(fg));
            tag.set_pixels_above_lines(8);
            self.buffer.tag_table().add(&tag);
            self.heading[idx] = Some(tag);
        }
        self.heading[idx].as_ref().unwrap()
    }

    fn bold_tag(&mut self) -> &TextTag {
        if self.bold.is_none() {
            let tag = TextTag::new(Some("bold"));
            tag.set_weight(700);
            tag.set_foreground(Some(self.theme.ink));
            self.buffer.tag_table().add(&tag);
            self.bold = Some(tag);
        }
        self.bold.as_ref().unwrap()
    }

    fn italic_tag(&mut self) -> &TextTag {
        if self.italic.is_none() {
            let tag = TextTag::new(Some("italic"));
            tag.set_style(Style::Italic);
            tag.set_foreground(Some(self.theme.ink));
            self.buffer.tag_table().add(&tag);
            self.italic = Some(tag);
        }
        self.italic.as_ref().unwrap()
    }

    fn strike_tag(&mut self) -> &TextTag {
        if self.strike.is_none() {
            let tag = TextTag::new(Some("strike"));
            tag.set_strikethrough(true);
            tag.set_foreground(Some(self.theme.ink));
            self.buffer.tag_table().add(&tag);
            self.strike = Some(tag);
        }
        self.strike.as_ref().unwrap()
    }

    fn code_inline_tag(&mut self) -> &TextTag {
        if self.code_inline.is_none() {
            let tag = TextTag::new(Some("code-inline"));
            tag.set_family(Some("monospace"));
            tag.set_foreground(Some(self.theme.ink));
            tag.set_background(Some(self.theme.code_bg));
            self.buffer.tag_table().add(&tag);
            self.code_inline = Some(tag);
        }
        self.code_inline.as_ref().unwrap()
    }

    fn code_block_tag(&mut self) -> &TextTag {
        if self.code_block.is_none() {
            let tag = TextTag::new(Some("code-block"));
            tag.set_family(Some("monospace"));
            tag.set_foreground(Some(self.theme.ink));
            tag.set_background(Some(self.theme.code_bg));
            self.buffer.tag_table().add(&tag);
            self.code_block = Some(tag);
        }
        self.code_block.as_ref().unwrap()
    }

    fn code_lang_tag(&mut self) -> &TextTag {
        if self.code_lang.is_none() {
            let tag = TextTag::new(Some("code-lang"));
            tag.set_scale(0.8);
            tag.set_foreground(Some(self.theme.muted));
            self.buffer.tag_table().add(&tag);
            self.code_lang = Some(tag);
        }
        self.code_lang.as_ref().unwrap()
    }

    fn blockquote_tag(&mut self, depth: usize) -> &TextTag {
        let idx = depth.min(3);
        if self.blockquote[idx].is_none() {
            let margin = 16 * (idx as i32 + 1);
            let name = format!("blockquote-{idx}");
            let tag = TextTag::new(Some(&name));
            tag.set_left_margin(margin);
            tag.set_foreground(Some(self.theme.muted));
            self.buffer.tag_table().add(&tag);
            self.blockquote[idx] = Some(tag);
        }
        self.blockquote[idx].as_ref().unwrap()
    }

    fn para_spacing_tag(&mut self) -> &TextTag {
        if self.para_spacing.is_none() {
            let tag = TextTag::new(Some("para-spacing"));
            tag.set_pixels_above_lines(6);
            self.buffer.tag_table().add(&tag);
            self.para_spacing = Some(tag);
        }
        self.para_spacing.as_ref().unwrap()
    }

    fn hr_tag(&mut self) -> &TextTag {
        if self.hr.is_none() {
            let tag = TextTag::new(Some("hr"));
            tag.set_foreground(Some(self.theme.accent));
            self.buffer.tag_table().add(&tag);
            self.hr = Some(tag);
        }
        self.hr.as_ref().unwrap()
    }

    /// Build a named link tag encoding the href in the tag name.
    fn link_tag_for(&mut self, href: &str) -> TextTag {
        let name = format!("link|{href}");
        if let Some(existing) = self.buffer.tag_table().lookup(&name) {
            return existing;
        }
        let tag = TextTag::new(Some(&name));
        tag.set_foreground(Some(self.theme.accent));
        tag.set_underline(Underline::Single);
        self.buffer.tag_table().add(&tag);
        tag
    }
}

// ── list rendering helpers ────────────────────────────────────────────────────

fn bullet_char(depth: usize) -> &'static str {
    match depth % 3 {
        0 => "•",
        1 => "◦",
        _ => "▪",
    }
}

fn list_indent(depth: usize) -> i32 {
    16 * (depth as i32 + 1)
}

// ── Render context ────────────────────────────────────────────────────────────

struct RenderCtx<'a> {
    buffer: gtk::TextBuffer,
    view: &'a TextView,
    tags: &'a mut TagStore,
    task_idx: usize,
    on_toggle: Rc<dyn Fn(usize)>,
    base_dir: PathBuf,
    content_width: i32,
    color: String,
    link_map: HashMap<String, String>,
}

impl<'a> RenderCtx<'a> {
    fn newline(&self, iter: &mut gtk::TextIter) {
        self.buffer.insert(iter, "\n");
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Options for `render_view`.
pub struct RenderOpts {
    /// Base directory for resolving relative image paths.
    pub base_dir: PathBuf,
    /// Note colour: yellow|green|blue|pink|purple|gray|orange (default "yellow").
    pub color: String,
    /// Usable text width in px. Images wider than this are scaled down to fit the
    /// note width (preserving aspect); narrower images are shown at natural size.
    pub content_width: i32,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self { base_dir: PathBuf::from("."), color: "yellow".into(), content_width: DEFAULT_CONTENT_WIDTH }
    }
}

/// Fallback usable text width (≈ default note 280px minus 32px card padding) when
/// no real geometry width is supplied.
pub const DEFAULT_CONTENT_WIDTH: i32 = 248;

/// Build a read-only `gtk::TextView` rendering `doc` using theme colours.
/// `on_toggle(index)` is called when a task checkbox is toggled.
pub fn render_view(
    doc: &RenderDoc,
    opts: &RenderOpts,
    on_toggle: Rc<dyn Fn(usize)>,
) -> TextView {
    let t = theme::theme(&opts.color);
    let buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
    let view = TextView::builder()
        .buffer(&buffer)
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(WrapMode::Word)
        .left_margin(0)
        .right_margin(0)
        .top_margin(0)
        .bottom_margin(0)
        .build();

    let mut tags = TagStore::new(buffer.clone(), t);
    tags.body_tag();

    let mut ctx = RenderCtx {
        buffer: buffer.clone(),
        view: &view,
        tags: &mut tags,
        task_idx: 0,
        on_toggle,
        base_dir: opts.base_dir.clone(),
        content_width: opts.content_width,
        color: opts.color.clone(),
        link_map: HashMap::new(),
    };

    let mut iter = ctx.buffer.end_iter();
    let blocks = doc.blocks.clone();
    render_blocks(&mut ctx, &blocks, 0, &mut iter);
    trim_trailing_newline(&ctx.buffer);
    wire_link_click(&view, ctx.link_map);
    view
}

fn trim_trailing_newline(buffer: &gtk::TextBuffer) {
    let end = buffer.end_iter();
    if end.offset() == 0 {
        return;
    }
    let mut start = end;
    start.backward_char();
    if buffer.text(&start, &end, false) == "\n" {
        buffer.delete(&mut start.clone(), &mut end.clone());
    }
}

// ── Block renderer ────────────────────────────────────────────────────────────

fn render_blocks(ctx: &mut RenderCtx, blocks: &[Block], depth: usize, iter: &mut gtk::TextIter) {
    for block in blocks {
        render_block(ctx, block, depth, iter);
    }
}

fn render_block(ctx: &mut RenderCtx, block: &Block, depth: usize, iter: &mut gtk::TextIter) {
    match block {
        Block::Heading { level, inlines } => render_heading(ctx, *level, inlines, iter),
        Block::Paragraph(inlines) => render_paragraph(ctx, inlines, iter),
        Block::Code { lang, text } => render_code_block(ctx, lang.as_deref(), text, iter),
        Block::Quote(inner) => render_blockquote(ctx, inner, depth, iter),
        Block::List(list) => render_list(ctx, list, depth, iter),
        Block::Table(table) => render_table(ctx, table, iter),
        Block::ThematicBreak => render_hr(ctx, iter),
        // A preserved blank line: just an empty line, no paragraph spacing tag.
        Block::BlankLine => ctx.newline(iter),
    }
}

fn render_table(ctx: &mut RenderCtx, table: &Table, iter: &mut gtk::TextIter) {
    let columns = table.alignments.len().max(1) as i32;
    let width = ctx.content_width.max(1);
    let cell_width = (width / columns - 12).max(80);
    let grid = gtk::Grid::builder().column_homogeneous(true).build();
    for (row, cells) in std::iter::once(&table.header).chain(&table.rows).enumerate() {
        for (col, inlines) in cells.iter().enumerate() {
            let opts = RenderOpts {
                base_dir: ctx.base_dir.clone(),
                color: ctx.color.clone(),
                content_width: cell_width,
            };
            let cell = render_view(&RenderDoc { blocks: vec![Block::Paragraph(inlines.clone())] }, &opts, ctx.on_toggle.clone());
            cell.set_wrap_mode(WrapMode::WordChar);
            cell.set_width_request(cell_width);
            cell.set_hexpand(true);
            cell.add_css_class("waynote-table-cell");
            let buffer = cell.buffer();
            let alignment = gtk::TextTag::new(None);
            alignment.set_justification(match table.alignments.get(col) {
                Some(pulldown_cmark::Alignment::Center) => gtk::Justification::Center,
                Some(pulldown_cmark::Alignment::Right) => gtk::Justification::Right,
                _ => gtk::Justification::Left,
            });
            if row == 0 { alignment.set_weight(700); }
            buffer.tag_table().add(&alignment);
            buffer.apply_tag(&alignment, &buffer.start_iter(), &buffer.end_iter());
            grid.attach(&cell, col as i32, row as i32, 1, 1);
        }
    }
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Never)
        .width_request(width)
        .child(&grid)
        .build();
    let anchor = ctx.buffer.create_child_anchor(iter);
    ctx.view.add_child_at_anchor(&scroll, &anchor);
    ctx.newline(iter);
}

// ── Heading ───────────────────────────────────────────────────────────────────

fn render_heading(ctx: &mut RenderCtx, level: u8, inlines: &[Inline], iter: &mut gtk::TextIter) {
    let start_off = iter.offset();
    render_inlines_with_base(ctx, inlines, iter, None);
    ctx.newline(iter);
    let tag = ctx.tags.heading_tag(level).clone();
    apply_over_range(&ctx.buffer, start_off, iter.offset() - 1, &tag);
    if level == 1 {
        let ul = ctx.tags.h1_underline_tag().clone();
        apply_over_range(&ctx.buffer, start_off, iter.offset() - 1, &ul);
    }
}

// ── Paragraph ─────────────────────────────────────────────────────────────────

fn render_paragraph(ctx: &mut RenderCtx, inlines: &[Inline], iter: &mut gtk::TextIter) {
    let start_off = iter.offset();
    render_inlines_with_base(ctx, inlines, iter, None);
    ctx.newline(iter);
    let tag = ctx.tags.para_spacing_tag().clone();
    apply_over_range(&ctx.buffer, start_off, start_off + 1, &tag);
}

// ── Code block ───────────────────────────────────────────────────────────────

fn render_code_block(
    ctx: &mut RenderCtx,
    lang: Option<&str>,
    text: &str,
    iter: &mut gtk::TextIter,
) {
    if let Some(lang) = lang {
        let start_off = iter.offset();
        ctx.buffer.insert(iter, lang);
        ctx.newline(iter);
        let lang_tag = ctx.tags.code_lang_tag().clone();
        apply_over_range(&ctx.buffer, start_off, iter.offset() - 1, &lang_tag);
    }
    let start_off = iter.offset();
    ctx.buffer.insert(iter, text);
    ctx.newline(iter);
    let mono = ctx.tags.code_block_tag().clone();
    apply_over_range(&ctx.buffer, start_off, iter.offset() - 1, &mono);
}

// ── Blockquote ────────────────────────────────────────────────────────────────

fn render_blockquote(
    ctx: &mut RenderCtx,
    inner: &[Block],
    depth: usize,
    iter: &mut gtk::TextIter,
) {
    let start_off = iter.offset();
    render_blocks(ctx, inner, depth + 1, iter);
    let end_off = iter.offset();
    let tag = ctx.tags.blockquote_tag(depth).clone();
    apply_over_range(&ctx.buffer, start_off, end_off, &tag);
}

// ── Thematic break ────────────────────────────────────────────────────────────

fn render_hr(ctx: &mut RenderCtx, iter: &mut gtk::TextIter) {
    let start_off = iter.offset();
    ctx.buffer.insert(iter, "────────────────────");
    ctx.newline(iter);
    let tag = ctx.tags.hr_tag().clone();
    apply_over_range(&ctx.buffer, start_off, iter.offset() - 1, &tag);
}

// ── List ─────────────────────────────────────────────────────────────────────

fn render_list(ctx: &mut RenderCtx, list: &List, depth: usize, iter: &mut gtk::TextIter) {
    for (i, item) in list.items.iter().enumerate() {
        let number = list.start as usize + i;
        render_list_item(ctx, item, list.ordered, number, depth, iter);
    }
}

fn render_list_item(
    ctx: &mut RenderCtx,
    item: &ListItem,
    ordered: bool,
    number: usize,
    depth: usize,
    iter: &mut gtk::TextIter,
) {
    let start_off = iter.offset();
    match item.task {
        Some(checked) => render_task_item(ctx, item, checked, depth, iter),
        None => render_regular_item(ctx, item, ordered, number, depth, iter),
    }
    // Apply the depth's left-margin tag over the whole item so nested lists indent.
    let margin_tag = ctx.tags.margin_tag(list_indent(depth));
    apply_over_range(&ctx.buffer, start_off, iter.offset(), &margin_tag);
}

fn render_task_item(
    ctx: &mut RenderCtx,
    item: &ListItem,
    checked: bool,
    depth: usize,
    iter: &mut gtk::TextIter,
) {
    let task_idx = ctx.task_idx;
    ctx.task_idx += 1;

    let anchor = ctx.buffer.create_child_anchor(iter);
    let cb = CheckButton::new();
    cb.set_active(checked);
    cb.set_can_focus(false);
    let on_toggle = ctx.on_toggle.clone();
    cb.connect_toggled(move |_| on_toggle(task_idx));
    ctx.view.add_child_at_anchor(&cb, &anchor);

    let text_start = iter.offset();
    render_item_content(ctx, &item.blocks, depth, iter);
    // A completed task reads as "done": strike through + dim its text. Applied as a
    // range overlay AFTER rendering, so it composes over any inline tags (bold,
    // links) and never touches the CheckButton or its toggle handler.
    if checked {
        apply_completed_task_style(ctx, text_start, iter.offset());
    }
}

/// Overlay strikethrough + muted colour on a completed task's text range. `muted`
/// is bumped to top priority so its foreground wins over `strike`'s ink colour and
/// any inline foreground (link/code), giving a uniformly dimmed, struck line.
fn apply_completed_task_style(ctx: &mut RenderCtx, start_off: i32, end_off: i32) {
    if start_off >= end_off {
        return;
    }
    let strike = ctx.tags.strike_tag().clone();
    let muted = ctx.tags.muted_tag().clone();
    let top_priority = ctx.buffer.tag_table().size().saturating_sub(1);
    muted.set_priority(top_priority);
    apply_over_range(&ctx.buffer, start_off, end_off, &strike);
    apply_over_range(&ctx.buffer, start_off, end_off, &muted);
}

fn render_regular_item(
    ctx: &mut RenderCtx,
    item: &ListItem,
    ordered: bool,
    number: usize,
    depth: usize,
    iter: &mut gtk::TextIter,
) {
    let bullet = if ordered {
        format!("{number}. ")
    } else {
        format!("{} ", bullet_char(depth))
    };
    let accent = ctx.tags.accent_tag().clone();
    ctx.buffer.insert_with_tags(iter, &bullet, &[&accent]);
    render_item_content(ctx, &item.blocks, depth, iter);
}

fn render_item_content(
    ctx: &mut RenderCtx,
    blocks: &[Block],
    depth: usize,
    iter: &mut gtk::TextIter,
) {
    for block in blocks {
        match block {
            Block::Paragraph(inlines) => {
                render_inlines_with_base(ctx, inlines, iter, None);
                ctx.newline(iter);
            }
            _ => render_block(ctx, block, depth + 1, iter),
        }
    }
}

// ── Inline renderer ───────────────────────────────────────────────────────────

fn render_inlines_with_base(
    ctx: &mut RenderCtx,
    inlines: &[Inline],
    iter: &mut gtk::TextIter,
    extra: Option<&[TextTag]>,
) {
    for inline in inlines {
        render_inline(ctx, inline, iter, extra);
    }
}

fn render_inline(
    ctx: &mut RenderCtx,
    inline: &Inline,
    iter: &mut gtk::TextIter,
    extra: Option<&[TextTag]>,
) {
    match inline {
        Inline::Text(s) => insert_text_inline(ctx, s, iter, extra),
        // Render a soft break (a single newline in the source) as a VISIBLE line
        // break, not the CommonMark default of a space. A sticky note is a WYSIWYG
        // surface: every Enter the user typed must show up as a line break (this
        // matches Obsidian's default "strict line breaks: off" reading view). A
        // paragraph break (blank line) still yields the larger inter-paragraph gap.
        Inline::SoftBreak => ctx.newline(iter),
        Inline::HardBreak => ctx.newline(iter),
        Inline::Code(s) => render_inline_code(ctx, s, iter),
        Inline::Emphasis(children) => render_emphasis(ctx, children, iter, extra),
        Inline::Strong(children) => render_strong(ctx, children, iter, extra),
        Inline::Strikethrough(children) => render_strikethrough(ctx, children, iter, extra),
        Inline::Link { href, children } => render_link(ctx, href, children, iter),
        Inline::Image { dest, alt } => render_image(ctx, dest, alt, iter),
    }
}

fn insert_text_inline(
    ctx: &mut RenderCtx,
    text: &str,
    iter: &mut gtk::TextIter,
    extra: Option<&[TextTag]>,
) {
    let body = ctx.tags.body.as_ref().unwrap().clone();
    let mut all: Vec<&TextTag> = vec![&body];
    if let Some(tags) = extra {
        all.extend(tags.iter());
    }
    ctx.buffer.insert_with_tags(iter, text, &all);
}

fn render_inline_code(ctx: &mut RenderCtx, text: &str, iter: &mut gtk::TextIter) {
    let mono = ctx.tags.code_inline_tag().clone();
    ctx.buffer.insert_with_tags(iter, text, &[&mono]);
}

fn render_emphasis(
    ctx: &mut RenderCtx,
    children: &[Inline],
    iter: &mut gtk::TextIter,
    extra: Option<&[TextTag]>,
) {
    let it_tag = ctx.tags.italic_tag().clone();
    let start_off = iter.offset();
    render_inlines_with_base(ctx, children, iter, extra);
    apply_over_range(&ctx.buffer, start_off, iter.offset(), &it_tag);
}

fn render_strong(
    ctx: &mut RenderCtx,
    children: &[Inline],
    iter: &mut gtk::TextIter,
    extra: Option<&[TextTag]>,
) {
    let bold_tag = ctx.tags.bold_tag().clone();
    let start_off = iter.offset();
    render_inlines_with_base(ctx, children, iter, extra);
    apply_over_range(&ctx.buffer, start_off, iter.offset(), &bold_tag);
}

fn render_strikethrough(
    ctx: &mut RenderCtx,
    children: &[Inline],
    iter: &mut gtk::TextIter,
    extra: Option<&[TextTag]>,
) {
    let st_tag = ctx.tags.strike_tag().clone();
    let start_off = iter.offset();
    render_inlines_with_base(ctx, children, iter, extra);
    apply_over_range(&ctx.buffer, start_off, iter.offset(), &st_tag);
}

fn render_link(
    ctx: &mut RenderCtx,
    href: &str,
    children: &[Inline],
    iter: &mut gtk::TextIter,
) {
    let link_tag = ctx.tags.link_tag_for(href);
    let start_off = iter.offset();
    render_inlines_with_base(ctx, children, iter, None);
    apply_over_range(&ctx.buffer, start_off, iter.offset(), &link_tag);
    ctx.link_map.insert(format!("link|{href}"), href.to_string());
}

fn render_image(ctx: &mut RenderCtx, dest: &str, alt: &str, iter: &mut gtk::TextIter) {
    match classify_image(dest) {
        ImageSrc::LocalRelative(rel) => {
            let path = ctx.base_dir.join(&rel);
            insert_image_or_placeholder(ctx, &path, alt, iter);
        }
        ImageSrc::LocalAbsolute(abs) => {
            insert_image_or_placeholder(ctx, Path::new(&abs), alt, iter);
        }
        ImageSrc::Remote | ImageSrc::Unsupported => {
            insert_alt_text(ctx, alt, iter);
        }
    }
}

fn insert_image_or_placeholder(
    ctx: &mut RenderCtx,
    path: &Path,
    alt: &str,
    iter: &mut gtk::TextIter,
) {
    let file = gio::File::for_path(path);
    match gdk::Texture::from_file(&file) {
        Ok(texture) => {
            // Fit the image to the note's usable content width (the dominant
            // pattern in Keep/Apple Notes/Notion): scale DOWN to the note width
            // when larger, never upscale beyond the natural size. This replaced a
            // fixed 240px cap that made images look tiny in any note wider than the
            // default — now they track the actual note width across re-renders.
            // An explicit height is REQUIRED: a `can_shrink` Picture in a TextView
            // child anchor collapses to height 0 (invisible) without one.
            // `ContentFit::ScaleDown` lets the image shrink (keeping aspect) if the
            // note is later narrower than the request, instead of clipping.
            let tw = texture.width().max(1);
            let th = texture.height().max(1);
            let target_w = tw.min(ctx.content_width.max(1));
            let target_h = ((target_w as i64 * th as i64) / tw as i64).max(1) as i32;
            let pic = Picture::for_paintable(&texture);
            pic.set_can_shrink(true);
            pic.set_content_fit(gtk::ContentFit::ScaleDown);
            pic.set_size_request(target_w, target_h);
            // Click → open the image at full size in the system viewer (the
            // click-to-full-size pattern that complements fit-to-width). A pointer
            // cursor hints it is clickable.
            pic.set_cursor_from_name(Some("pointer"));
            let full_path = path.to_string_lossy().to_string();
            let click = GestureClick::new();
            click.set_button(1);
            click.connect_released(move |_g, _n, _x, _y| open_url(&full_path));
            pic.add_controller(click);
            let anchor = ctx.buffer.create_child_anchor(iter);
            ctx.view.add_child_at_anchor(&pic, &anchor);
        }
        Err(_) => insert_alt_text(ctx, alt, iter),
    }
}

fn insert_alt_text(ctx: &mut RenderCtx, alt: &str, iter: &mut gtk::TextIter) {
    let text = if alt.is_empty() { "[image]" } else { alt };
    let muted = ctx.tags.muted_tag().clone();
    ctx.buffer.insert_with_tags(iter, text, &[&muted]);
}

// ── Link click wiring ─────────────────────────────────────────────────────────

fn wire_link_click(view: &TextView, link_map: HashMap<String, String>) {
    let gesture = GestureClick::new();
    gesture.set_button(1);
    let v = view.clone();
    let map = Rc::new(link_map);
    gesture.connect_released(move |_g, _n_press, x, y| {
        handle_link_click(&v, &map, x, y);
    });
    view.add_controller(gesture);
}

fn handle_link_click(view: &TextView, map: &HashMap<String, String>, x: f64, y: f64) {
    if view.buffer().has_selection() { return; }
    use gtk::TextWindowType;
    let (bx, by) = view.window_to_buffer_coords(TextWindowType::Widget, x as i32, y as i32);
    let Some(iter) = view.iter_at_location(bx, by) else { return };
    for tag in iter.tags() {
        let Some(name) = tag.name() else { continue };
        if let Some(href) = map.get(name.as_str()) {
            open_url(href);
            return;
        }
    }
}

fn open_url(href: &str) {
    let allowed = is_web_link(href)
        || href.starts_with("file://")
        || href.starts_with('/')
        || !href.contains(':');
    if allowed {
        let _ = std::process::Command::new("xdg-open").arg(href).spawn();
    }
}

// ── Utility ───────────────────────────────────────────────────────────────────

fn apply_over_range(buffer: &gtk::TextBuffer, start_off: i32, end_off: i32, tag: &TextTag) {
    if start_off >= end_off {
        return;
    }
    let s = buffer.iter_at_offset(start_off);
    let e = buffer.iter_at_offset(end_off);
    buffer.apply_tag(tag, &s, &e);
}

