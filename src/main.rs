use std::{
    env,
    error::Error,
    fs,
    io::{self, stdout},
    path::{Path, PathBuf},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use pulldown_cmark::{
    Alignment, CodeBlockKind, Event as MarkdownEvent, HeadingLevel, Options,
    Parser as MarkdownParser, Tag, TagEnd,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

// --- アプリケーションの状態管理 ---

enum AppMode {
    Explorer,
    Preview,
}

struct ExplorerState {
    current_path: PathBuf,
    entries: Vec<PathBuf>,
    list_state: ListState,
    error_message: Option<String>,
    command_input: String,
    in_command_mode: bool,
}

impl ExplorerState {
    fn new() -> io::Result<Self> {
        let mut state = Self {
            current_path: env::current_dir()?,
            entries: Vec::new(),
            list_state: ListState::default(),
            error_message: None,
            command_input: String::new(),
            in_command_mode: false,
        };
        state.load_entries()?;
        Ok(state)
    }

    /// ディレクトリ読み込み時にカーソル位置を必ずリセットする
    fn load_entries(&mut self) -> io::Result<()> {
        let mut entries = fs::read_dir(&self.current_path)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();

        entries.sort_by(|a, b| {
            let a_is_dir = a.is_dir();
            let b_is_dir = b.is_dir();
            a_is_dir.cmp(&b_is_dir).reverse().then_with(|| a.cmp(b))
        });

        self.entries = entries;

        if !self.entries.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
        Ok(())
    }

    fn next(&mut self) {
        if self.entries.is_empty() { return; }
        let i = self.list_state.selected().map_or(0, |i| {
            if i >= self.entries.len() - 1 { 0 } else { i + 1 }
        });
        self.list_state.select(Some(i));
    }

    fn previous(&mut self) {
        if self.entries.is_empty() { return; }
        let i = self.list_state.selected().map_or(0, |i| {
            if i == 0 { self.entries.len() - 1 } else { i - 1 }
        });
        self.list_state.select(Some(i));
    }
}

struct PreviewState {
    content: Text<'static>,
    scroll: u16,
    title: String,
}

impl PreviewState {
    fn new(file_path: &Path) -> io::Result<Self> {
        let original_markdown = fs::read_to_string(file_path)?;
        let placeholder = "[[BR_TAG]]";
        let processed_markdown = original_markdown
            .replace("<br>", placeholder)
            .replace("<BR>", placeholder);
        let content = render_markdown(&processed_markdown, placeholder);

        Ok(Self {
            content,
            scroll: 0,
            title: file_path.to_string_lossy().to_string(),
        })
    }

    fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    fn scroll_down(&mut self, frame_height: u16) {
        let max_scroll = self
            .content
            .height()
            .saturating_sub(frame_height as usize) as u16;
        if self.scroll < max_scroll {
            self.scroll = self.scroll.saturating_add(1);
        }
    }
}

// --- メインロジック ---

fn main() -> Result<(), Box<dyn Error>> {
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal);
    restore_terminal()?;

    if let Err(err) = result {
        if err.to_string() != "quit" {
            println!("エラーが発生しました: {}", err);
        }
    }
    Ok(())
}

fn run<B: Backend>(terminal: &mut Terminal<B>) -> io::Result<()> {
    let mut mode = AppMode::Explorer;
    let mut explorer_state = ExplorerState::new()?;
    let mut preview_state: Option<PreviewState> = None;

    loop {
        terminal.draw(|f| match mode {
            AppMode::Explorer => ui_explorer(f, &mut explorer_state),
            AppMode::Preview => {
                if let Some(state) = &mut preview_state {
                    ui_preview(f, state);
                }
            }
        })?;

        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match mode {
                AppMode::Preview => {
                    if let Some(state) = &mut preview_state {
                        let frame_height = terminal.size()?.height;
                        match key.code {
                            KeyCode::Char('q') => {
                                preview_state = None;
                                mode = AppMode::Explorer;
                            }
                            KeyCode::Up | KeyCode::Char('k') => state.scroll_up(),
                            KeyCode::Down | KeyCode::Char('j') => state.scroll_down(frame_height),
                            _ => {}
                        }
                    }
                }
                AppMode::Explorer => {
                    if explorer_state.in_command_mode {
                        match key.code {
                            KeyCode::Enter => {
                                if explorer_state.command_input == "q" {
                                    return Err(io::Error::new(io::ErrorKind::Other, "quit"));
                                }
                                explorer_state.command_input.clear();
                                explorer_state.in_command_mode = false;
                            }
                            KeyCode::Char(c) => explorer_state.command_input.push(c),
                            KeyCode::Backspace => {
                                explorer_state.command_input.pop();
                            }
                            KeyCode::Esc => {
                                explorer_state.command_input.clear();
                                explorer_state.in_command_mode = false;
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char(':') => {
                                explorer_state.in_command_mode = true;
                            }
                            KeyCode::Down | KeyCode::Char('j') => explorer_state.next(),
                            KeyCode::Up | KeyCode::Char('k') => explorer_state.previous(),
                            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                                if let Some(parent) = explorer_state.current_path.parent() {
                                    explorer_state.current_path = parent.to_path_buf();
                                    explorer_state.load_entries()?;
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(selected_index) = explorer_state.list_state.selected() {
                                    if let Some(selected_path) = explorer_state.entries.get(selected_index) {
                                        let selected_path = selected_path.clone();
                                        if selected_path.is_dir() {
                                            explorer_state.current_path = dunce::canonicalize(selected_path)?;
                                            explorer_state.load_entries()?;
                                        } else {
                                            if selected_path.extension().and_then(|s| s.to_str()) == Some("md") {
                                                match PreviewState::new(&selected_path) {
                                                    Ok(state) => {
                                                        preview_state = Some(state);
                                                        mode = AppMode::Preview;
                                                    }
                                                    Err(e) => {
                                                        explorer_state.error_message = Some(format!("プレビューを開けません: {}", e));
                                                    }
                                                }
                                            } else {
                                                explorer_state.error_message = Some("Markdownファイル以外はプレビューできません。".to_string());
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}


// --- UI描画 ---

fn ui_explorer(f: &mut Frame, state: &mut ExplorerState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(2)].as_ref())
        .split(f.size());

    // ★最重要修正点: unwrap()を完全に排除
    let items: Vec<ListItem> = state
        .entries
        .iter()
        .map(|path| {
            // path.file_name()がNoneの場合でもpanicせず、安全にデフォルト値("..")を使う
            let file_name = path
                .file_name()
                .map_or_else(|| "..".into(), |s| s.to_string_lossy());

            let display_name = if path.is_dir() {
                format!("{}/", file_name)
            } else {
                file_name.to_string()
            };

            let style = if path.is_dir() {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            ListItem::new(Span::styled(display_name, style))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(state.current_path.to_string_lossy().to_string()),
        )
        .highlight_style(
            Style::default()
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, chunks[0], &mut state.list_state);

    let status_text = if state.in_command_mode {
        format!(":{}", state.command_input)
    } else if let Some(err) = &state.error_message {
        err.clone()
    } else {
        "j/k or ↓/↑: Move | Enter: Open | h or Backspace: Up | :q Enter: Quit".to_string()
    };
    let status_bar = Paragraph::new(status_text).style(if state.error_message.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    });

    f.render_widget(status_bar, chunks[1]);
}

fn ui_preview(f: &mut Frame, state: &mut PreviewState) {
    let area = f.size();
    let paragraph = Paragraph::new(state.content.clone())
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Previewing: {} (Press 'q' to close)",
            state.title
        )))
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .wrap(Wrap { trim: false })
        .scroll((state.scroll, 0));
    f.render_widget(paragraph, area);
}

// --- ターミナル設定 ---
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

// --- Markdownレンダリング ---
fn render_markdown(markdown_input: &str, br_placeholder: &str) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut list_stack: Vec<u64> = Vec::new();
    let mut table_alignments: Vec<Alignment> = Vec::new();
    let mut in_table_header = false;
    let mut in_code_block = false;

    let parser = MarkdownParser::new_ext(markdown_input, Options::all());
    for event in parser {
        match event {
            MarkdownEvent::Start(tag) => {
                let current_style = *style_stack.last().unwrap_or(&Style::default());
                match tag {
                    Tag::Heading { level, .. } => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        lines.push(Line::default());
                        let style =
                            Style::default()
                                .add_modifier(Modifier::BOLD)
                                .fg(match level {
                                    HeadingLevel::H1 => Color::LightRed,
                                    HeadingLevel::H2 => Color::LightYellow,
                                    _ => Color::LightCyan,
                                });
                        style_stack.push(style);
                    }
                    Tag::BlockQuote => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        let style = Style::default()
                            .fg(Color::Gray)
                            .add_modifier(Modifier::ITALIC);
                        style_stack.push(style);
                    }
                    Tag::CodeBlock(kind) => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        lines.push(Line::default());
                        in_code_block = true;
                        let lang = match kind {
                            CodeBlockKind::Fenced(lang) => lang.into_string(),
                            CodeBlockKind::Indented => String::new(),
                        };
                        let border_style = Style::default().fg(Color::DarkGray);
                        lines.push(Line::from(vec![
                            Span::styled("┌─── ".to_string(), border_style),
                            Span::styled(lang, Style::default().fg(Color::Yellow)),
                        ]));
                        style_stack.push(Style::default().bg(Color::Rgb(30, 30, 30)));
                    }
                    Tag::Table(aligns) => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        let mut top_border =
                            vec![Span::styled("┌".to_string(), Style::default().fg(Color::DarkGray))];
                        for _ in 0..aligns.len() {
                            top_border.push(Span::styled(
                                "────────".to_string(),
                                Style::default().fg(Color::DarkGray),
                            ));
                            top_border.push(Span::styled(
                                "┬".to_string(),
                                Style::default().fg(Color::DarkGray),
                            ));
                        }
                        if !top_border.is_empty() {
                            top_border.pop();
                        }
                        top_border.push(Span::styled(
                            "┐".to_string(),
                            Style::default().fg(Color::DarkGray),
                        ));
                        lines.push(Line::from(top_border));

                        table_alignments = aligns;
                    }
                    Tag::TableHead => {
                        in_table_header = true;
                    }
                    Tag::TableRow => {
                        current_spans
                            .push(Span::styled("│ ".to_string(), Style::default().fg(Color::DarkGray)));
                    }
                    Tag::TableCell => { /* No action needed */ }
                    Tag::List(start_num) => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        list_stack.push(start_num.unwrap_or(1));
                    }
                    Tag::Item => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        let indent = "  ".repeat(list_stack.len().saturating_sub(1));
                        let marker = if let Some(num) = list_stack.last_mut() {
                            let m = format!("{}. ", *num);
                            *num += 1;
                            m
                        } else {
                            "• ".to_string()
                        };
                        current_spans.push(Span::raw(indent));
                        current_spans
                            .push(Span::styled(marker, Style::default().fg(Color::LightMagenta)));
                    }
                    Tag::Emphasis => {
                        style_stack.push(current_style.add_modifier(Modifier::ITALIC))
                    }
                    Tag::Strong => style_stack.push(current_style.add_modifier(Modifier::BOLD)),
                    Tag::Strikethrough => {
                        style_stack.push(current_style.add_modifier(Modifier::CROSSED_OUT))
                    }
                    Tag::Link { .. } => style_stack
                        .push(Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED)),
                    _ => {}
                }
            }
            MarkdownEvent::End(tag) => match tag {
                TagEnd::Heading(_) | TagEnd::BlockQuote | TagEnd::Item => {
                    if !current_spans.is_empty() {
                        lines.push(Line::from(std::mem::take(&mut current_spans)));
                    }
                    style_stack.pop();
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    lines.push(Line::from(Span::styled(
                        "└──────────────────".to_string(),
                        Style::default().fg(Color::DarkGray),
                    )));
                    lines.push(Line::default());
                    style_stack.pop();
                }
                TagEnd::Table => {
                    let mut bottom_border =
                        vec![Span::styled("└".to_string(), Style::default().fg(Color::DarkGray))];
                    for _ in 0..table_alignments.len() {
                        bottom_border.push(Span::styled(
                            "────────".to_string(),
                            Style::default().fg(Color::DarkGray),
                        ));
                        bottom_border.push(Span::styled(
                            "┴".to_string(),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    if !bottom_border.is_empty() {
                        bottom_border.pop();
                    }
                    bottom_border
                        .push(Span::styled("┘".to_string(), Style::default().fg(Color::DarkGray)));
                    lines.push(Line::from(bottom_border));

                    table_alignments.clear();
                    lines.push(Line::default());
                }
                TagEnd::TableHead => {
                    in_table_header = false;
                    let mut separator_spans =
                        vec![Span::styled("├".to_string(), Style::default().fg(Color::DarkGray))];
                    for align in &table_alignments {
                        let sep = match align {
                            Alignment::Left => " :------- ",
                            Alignment::Center => " :------: ",
                            Alignment::Right => " -------: ",
                            Alignment::None => " -------- ",
                        };
                        separator_spans
                            .push(Span::styled(sep.to_string(), Style::default().fg(Color::DarkGray)));
                        separator_spans
                            .push(Span::styled("┼".to_string(), Style::default().fg(Color::DarkGray)));
                    }
                    if !separator_spans.is_empty() {
                        separator_spans.pop();
                    }
                    separator_spans
                        .push(Span::styled("┤".to_string(), Style::default().fg(Color::DarkGray)));
                    lines.push(Line::from(separator_spans));
                }
                TagEnd::TableRow => {
                    if !current_spans.is_empty() {
                        lines.push(Line::from(std::mem::take(&mut current_spans)));
                    }
                }
                TagEnd::TableCell => {
                    current_spans.push(Span::styled(" │ ".to_string(), Style::default().fg(Color::DarkGray)));
                }
                TagEnd::List(_) => {
                    list_stack.pop();
                    lines.push(Line::default());
                }
                TagEnd::Paragraph => {
                    if !current_spans.is_empty() {
                        lines.push(Line::from(std::mem::take(&mut current_spans)));
                    }
                    lines.push(Line::default());
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                    style_stack.pop();
                }
                _ => {}
            },
            MarkdownEvent::Text(text) => {
                let style = *style_stack.last().unwrap_or(&Style::default());
                if in_code_block {
                    for line in text.lines() {
                        lines.push(Line::from(vec![
                            Span::styled("│ ".to_string(), Style::default().fg(Color::DarkGray)),
                            Span::styled(line.to_string(), style),
                        ]));
                    }
                } else {
                    let final_style = if in_table_header {
                        style.add_modifier(Modifier::BOLD)
                    } else {
                        style
                    };
                    if text.contains(br_placeholder) {
                        let mut last_pos = 0;
                        while let Some(placeholder_pos) = text[last_pos..].find(br_placeholder) {
                            let absolute_pos = last_pos + placeholder_pos;
                            let before = &text[last_pos..absolute_pos];
                            if !before.is_empty() {
                                current_spans.push(Span::styled(before.to_string(), final_style));
                            }
                            current_spans
                                .push(Span::styled("<br>".to_string(), Style::default().fg(Color::Red)));
                            last_pos = absolute_pos + br_placeholder.len();
                        }
                        let remaining = &text[last_pos..];
                        if !remaining.is_empty() {
                            current_spans.push(Span::styled(remaining.to_string(), final_style));
                        }
                    } else {
                        current_spans.push(Span::styled(text.to_string(), final_style));
                    }
                }
            }
            MarkdownEvent::Html(html) => {
                current_spans.push(Span::raw(html.to_string()));
            }
            MarkdownEvent::Code(text) => {
                let style = Style::default().fg(Color::Yellow).bg(Color::DarkGray);
                current_spans.push(Span::styled(format!("`{}`", text), style));
            }
            MarkdownEvent::HardBreak => {
                if !current_spans.is_empty() {
                    lines.push(Line::from(std::mem::take(&mut current_spans)));
                }
            }
            MarkdownEvent::SoftBreak => current_spans.push(Span::raw(" ".to_string())),
            MarkdownEvent::Rule => {
                if !current_spans.is_empty() {
                    lines.push(Line::from(std::mem::take(&mut current_spans)));
                }
                lines.push(Line::from(Span::styled(
                    "─".repeat(80),
                    Style::default().fg(Color::Gray),
                )));
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

