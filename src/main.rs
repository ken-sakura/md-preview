use std::{
    error::Error,
    fs,
    io::{self, stdout},
    time::Duration,
};

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use pulldown_cmark::{Alignment, CodeBlockKind, Event as MarkdownEvent, HeadingLevel, Options, Parser as MarkdownParser, Tag, TagEnd};
use ratatui::{
    prelude::*,
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
};

/// コマンドライン引数を定義する構造体
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// プレビューするMarkdownファイルのパス
    #[arg()]
    file_path: String,
}

/// アプリケーションの状態を保持する構造体
struct App<'a> {
    content: Text<'a>,
    scroll: u16,
}

impl<'a> App<'a> {
    fn new(content: Text<'a>) -> Self {
        Self { content, scroll: 0 }
    }

    fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    fn scroll_down(&mut self, frame_height: u16) {
        let max_scroll = self.content.height().saturating_sub(frame_height as usize) as u16;
        if self.scroll < max_scroll {
            self.scroll = self.scroll.saturating_add(1);
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let original_markdown = fs::read_to_string(&args.file_path)
        .map_err(|e| format!("ファイル '{}' が読み込めませんでした: {}", args.file_path, e))?;

    // ★回避策: pulldown-cmarkがテーブル内の<br>を消してしまうため、
    // 一時的なプレースホルダーに置き換える。大文字小文字を区別せずに置換。
    // ★修正点: プレースホルダーを短くする
    let placeholder = "[[BR_TAG]]";
    let processed_markdown = original_markdown.replace("<br>", placeholder).replace("<BR>", placeholder);

    let content = render_markdown(&processed_markdown, placeholder);

    let mut terminal = setup_terminal()?;
    let app = App::new(content);
    run_app(&mut terminal, app)?;
    restore_terminal()?;

    Ok(())
}

/// Markdown文字列を解析し、ratatuiのスタイル付きTextオブジェクトに変換する
fn render_markdown<'a>(markdown_input: &'a str, br_placeholder: &str) -> Text<'a> {
    let mut lines: Vec<Line<'a>> = Vec::new();
    let mut current_spans: Vec<Span<'a>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut list_stack: Vec<u64> = Vec::new();
    
    let mut table_alignments: Vec<Alignment> = Vec::new();
    let mut in_table_header = false;
    let mut in_code_block = false;

    let parser = MarkdownParser::new_ext(markdown_input, Options::all());
    for event in parser {
        match event {
            MarkdownEvent::Start(tag) => {
                let current_style = *style_stack.last().unwrap();
                match tag {
                    Tag::Heading { level, .. } => {
                        if !current_spans.is_empty() { lines.push(Line::from(std::mem::take(&mut current_spans))); }
                        lines.push(Line::default());
                        let style = Style::default().add_modifier(Modifier::BOLD).fg(match level {
                            HeadingLevel::H1 => Color::LightRed,
                            HeadingLevel::H2 => Color::LightYellow,
                            _ => Color::LightCyan,
                        });
                        style_stack.push(style);
                    }
                    Tag::BlockQuote => {
                        if !current_spans.is_empty() { lines.push(Line::from(std::mem::take(&mut current_spans))); }
                        let style = Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC);
                        style_stack.push(style);
                    }
                    Tag::CodeBlock(kind) => {
                        if !current_spans.is_empty() { lines.push(Line::from(std::mem::take(&mut current_spans))); }
                        lines.push(Line::default());
                        in_code_block = true;
                        let lang = match kind {
                            CodeBlockKind::Fenced(lang) => lang.into_string(),
                            CodeBlockKind::Indented => String::new(),
                        };
                        let border_style = Style::default().fg(Color::DarkGray);
                        lines.push(Line::from(vec![
                            Span::styled("┌─── ", border_style),
                            Span::styled(lang, Style::default().fg(Color::Yellow)),
                        ]));
                        style_stack.push(Style::default().bg(Color::Rgb(30, 30, 30)));
                    }
                    Tag::Table(aligns) => {
                        if !current_spans.is_empty() { lines.push(Line::from(std::mem::take(&mut current_spans))); }
                        let mut top_border = vec![Span::styled("┌", Style::default().fg(Color::DarkGray))];
                        for _ in 0..aligns.len() {
                            top_border.push(Span::styled("────────", Style::default().fg(Color::DarkGray)));
                            top_border.push(Span::styled("┬", Style::default().fg(Color::DarkGray)));
                        }
                        if !top_border.is_empty() { top_border.pop(); }
                        top_border.push(Span::styled("┐", Style::default().fg(Color::DarkGray)));
                        lines.push(Line::from(top_border));

                        table_alignments = aligns;
                    }
                    Tag::TableHead => {
                        in_table_header = true;
                    }
                    Tag::TableRow => {
                        current_spans.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
                    }
                    Tag::TableCell => { /* No action needed */ }
                    Tag::List(start_num) => {
                        if !current_spans.is_empty() { lines.push(Line::from(std::mem::take(&mut current_spans))); }
                        list_stack.push(start_num.unwrap_or(1));
                    }
                    Tag::Item => {
                        if !current_spans.is_empty() { lines.push(Line::from(std::mem::take(&mut current_spans))); }
                        let indent = "  ".repeat(list_stack.len().saturating_sub(1));
                        let marker = if let Some(num) = list_stack.last_mut() {
                            let m = format!("{}. ", *num); *num += 1; m
                        } else { "• ".to_string() };
                        current_spans.push(Span::raw(indent));
                        current_spans.push(Span::styled(marker, Style::default().fg(Color::LightMagenta)));
                    }
                    Tag::Emphasis => style_stack.push(current_style.add_modifier(Modifier::ITALIC)),
                    Tag::Strong => style_stack.push(current_style.add_modifier(Modifier::BOLD)),
                    Tag::Strikethrough => style_stack.push(current_style.add_modifier(Modifier::CROSSED_OUT)),
                    Tag::Link { .. } => style_stack.push(Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED)),
                    _ => {}
                }
            }
            MarkdownEvent::End(tag) => {
                 match tag {
                    TagEnd::Heading(_) | TagEnd::BlockQuote | TagEnd::Item => {
                        if !current_spans.is_empty() { lines.push(Line::from(std::mem::take(&mut current_spans))); }
                        style_stack.pop();
                    }
                    TagEnd::CodeBlock => {
                        in_code_block = false;
                        lines.push(Line::from(Span::styled("└──────────────────", Style::default().fg(Color::DarkGray))));
                        lines.push(Line::default());
                        style_stack.pop();
                    }
                    TagEnd::Table => {
                        let mut bottom_border = vec![Span::styled("└", Style::default().fg(Color::DarkGray))];
                        for _ in 0..table_alignments.len() {
                            bottom_border.push(Span::styled("────────", Style::default().fg(Color::DarkGray)));
                            bottom_border.push(Span::styled("┴", Style::default().fg(Color::DarkGray)));
                        }
                        if !bottom_border.is_empty() { bottom_border.pop(); }
                        bottom_border.push(Span::styled("┘", Style::default().fg(Color::DarkGray)));
                        lines.push(Line::from(bottom_border));
                        
                        table_alignments.clear();
                        lines.push(Line::default());
                    }
                    TagEnd::TableHead => {
                        in_table_header = false;
                        let mut separator_spans = vec![Span::styled("├", Style::default().fg(Color::DarkGray))];
                        for align in &table_alignments {
                            let sep = match align {
                                Alignment::Left => " :------- ",
                                Alignment::Center => " :------: ",
                                Alignment::Right => " -------: ",
                                Alignment::None => " -------- ",
                            };
                            separator_spans.push(Span::styled(sep, Style::default().fg(Color::DarkGray)));
                            separator_spans.push(Span::styled("┼", Style::default().fg(Color::DarkGray)));
                        }
                        if !separator_spans.is_empty() { separator_spans.pop(); }
                        separator_spans.push(Span::styled("┤", Style::default().fg(Color::DarkGray)));
                        lines.push(Line::from(separator_spans));
                    }
                    TagEnd::TableRow => {
                        if !current_spans.is_empty() { lines.push(Line::from(std::mem::take(&mut current_spans))); }
                    }
                    TagEnd::TableCell => {
                        current_spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
                    }
                    TagEnd::List(_) => {
                        list_stack.pop();
                        lines.push(Line::default());
                    }
                    TagEnd::Paragraph => {
                        if !current_spans.is_empty() { lines.push(Line::from(std::mem::take(&mut current_spans))); }
                        lines.push(Line::default());
                    }
                    TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                        style_stack.pop();
                    }
                    _ => {}
                }
            }
            MarkdownEvent::Text(text) => {
                if in_code_block {
                    let style = *style_stack.last().unwrap();
                    for line in text.lines() {
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                            Span::styled(line.to_string(), style),
                        ]));
                    }
                } else {
                    let style = *style_stack.last().unwrap();
                    let final_style = if in_table_header {
                        style.add_modifier(Modifier::BOLD)
                    } else {
                        style
                    };
                    
                    // ★回避策: プレースホルダーを<br>タグに戻す
                    if text.contains(br_placeholder) {
                        let mut last_pos = 0;
                        while let Some(placeholder_pos) = text[last_pos..].find(br_placeholder) {
                            let absolute_pos = last_pos + placeholder_pos;
                            
                            let before = &text[last_pos..absolute_pos];
                            if !before.is_empty() {
                                current_spans.push(Span::styled(before.to_string(), final_style));
                            }
                            
                            current_spans.push(Span::styled("<br>", Style::default().fg(Color::Red)));
                            
                            last_pos = absolute_pos + br_placeholder.len();
                        }
                        
                        let remaining = &text[last_pos..];
                        if !remaining.is_empty() {
                            current_spans.push(Span::styled(remaining.to_string(), final_style));
                        }
                    } else {
                        current_spans.push(Span::styled(text, final_style));
                    }
                }
            }
            MarkdownEvent::Html(html) => {
                current_spans.push(Span::raw(html));
            }
            MarkdownEvent::Code(text) => {
                let style = Style::default().fg(Color::Yellow).bg(Color::DarkGray);
                current_spans.push(Span::styled(format!("`{}`", text), style));
            }
            MarkdownEvent::HardBreak => {
                if !current_spans.is_empty() { lines.push(Line::from(std::mem::take(&mut current_spans))); }
            }
            MarkdownEvent::SoftBreak => current_spans.push(Span::raw(" ")),
            MarkdownEvent::Rule => {
                if !current_spans.is_empty() { lines.push(Line::from(std::mem::take(&mut current_spans))); }
                lines.push(Line::from(Span::styled("─".repeat(80), Style::default().fg(Color::Gray))));
                lines.push(Line::default());
            }
            _ => {}
        }
    }
    if !current_spans.is_empty() {
        lines.push(Line::from(std::mem::take(&mut current_spans)));
    }
    Text::from(lines)
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>, Box<dyn Error>> {
    let mut stdout = stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal() -> Result<(), Box<dyn Error>> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                let frame_height = terminal.size()?.height;
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Up | KeyCode::Char('k') => app.scroll_up(),
                    KeyCode::Down | KeyCode::Char('j') => app.scroll_down(frame_height),
                    _ => {}
                }
            }
        }
    }
}

fn ui(frame: &mut Frame, app: &mut App) {
    let area = frame.size();
    let paragraph = Paragraph::new(app.content.clone())
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    frame.render_widget(paragraph, area);
}

