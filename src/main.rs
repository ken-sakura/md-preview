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
// pulldown_cmarkからhtmlモジュールをインポート
use pulldown_cmark::{
    html, Alignment as MarkdownAlignment, CodeBlockKind, Event as MarkdownEvent, HeadingLevel,
    Options, Parser as MarkdownParser, Tag, TagEnd,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

// --- 配色テーマ定義 ---
struct ColorScheme {
    bg: Color,
    fg: Color,
    selection_bg: Color,
    selection_fg: Color,
    comment: Color,
    link: Color,
    heading: Color,
    code_bg: Color,
    inline_code_bg: Color,
    quote_fg: Color,
    quote_border: Color,
    hr: Color,
}

const GITHUB_DARK_THEME: ColorScheme = ColorScheme {
    bg: Color::Rgb(13, 17, 23),         // #0d1117
    fg: Color::Rgb(201, 209, 217),      // #c9d1d9
    selection_bg: Color::Rgb(3, 34, 82), // A selection color
    selection_fg: Color::Rgb(201, 209, 217),
    comment: Color::Rgb(139, 148, 158), // #8b949e
    link: Color::Rgb(88, 166, 255),     // #58a6ff
    heading: Color::Rgb(88, 166, 255),  // Using link color for headings
    code_bg: Color::Rgb(22, 27, 34),    // #161b22
    inline_code_bg: Color::Rgb(40, 45, 53),
    quote_fg: Color::Rgb(139, 148, 158), // #8b949e
    quote_border: Color::Rgb(48, 54, 61), // #30363d
    hr: Color::Rgb(33, 38, 45),         // #21262d
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
    char_count: usize,
}

impl PreviewState {
    fn new(file_path: &Path, theme: &ColorScheme) -> io::Result<Self> {
        let original_markdown = fs::read_to_string(file_path)?;
        let char_count = original_markdown.chars().count();
        let placeholder = "[[BR_TAG]]";
        let processed_markdown = original_markdown
            .replace("<br>", placeholder)
            .replace("<BR>", placeholder);
        let content = render_markdown(&processed_markdown, placeholder, theme);

        Ok(Self {
            content,
            scroll: 0,
            title: file_path.to_string_lossy().to_string(),
            char_count,
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
    // TUIモードの起動
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal);
    restore_terminal()?;

    if let Err(err) = result {
        // "quit"エラーはユーザーによる正常終了なので、エラーメッセージは表示しない
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
    let theme = &GITHUB_DARK_THEME;

    loop {
        terminal.draw(|f| match mode {
            AppMode::Explorer => ui_explorer(f, &mut explorer_state, theme),
            AppMode::Preview => {
                if let Some(state) = &mut preview_state {
                    ui_preview(f, state, theme);
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
                                let command_text = explorer_state.command_input.trim().to_string();
                                explorer_state.command_input.clear();
                                explorer_state.in_command_mode = false;
                                explorer_state.error_message = None; // コマンド実行時にエラーをクリア

                                let parts: Vec<&str> = command_text.split_whitespace().collect();

                                match parts.as_slice() {
                                    ["q"] => {
                                        return Err(io::Error::new(io::ErrorKind::Other, "quit"));
                                    }
                                    ["hp", filename] => {
                                        let file_path = explorer_state.current_path.join(filename);
                                        if !file_path.is_file() {
                                            explorer_state.error_message = Some(format!("ファイルが見つかりません: {}", filename));
                                            continue;
                                        }

                                        match fs::read_to_string(&file_path) {
                                            Ok(markdown_input) => {
                                                // MarkdownをHTMLに変換
                                                let parser = MarkdownParser::new(&markdown_input);
                                                let mut html_output = String::new();
                                                html::push_html(&mut html_output, parser);
                                                
                                                let char_count = html_output.chars().count();
                                                let content = Text::from(html_output);
                                                let title = format!("HTML Preview: {}", file_path.to_string_lossy());

                                                preview_state = Some(PreviewState {
                                                    content,
                                                    scroll: 0,
                                                    title,
                                                    char_count,
                                                });
                                                mode = AppMode::Preview;
                                            }
                                            Err(e) => {
                                                explorer_state.error_message = Some(format!("ファイル読み込みエラー: {}", e));
                                            }
                                        }
                                    }
                                    [] => {} // 空のコマンドは無視
                                    _ => {
                                        explorer_state.error_message = Some(format!("不明なコマンドです: {}", command_text));
                                    }
                                }
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
                        explorer_state.error_message = None; // 操作時にエラーをクリア
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
                                                match PreviewState::new(&selected_path, theme) {
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

fn ui_explorer(f: &mut Frame, state: &mut ExplorerState, theme: &ColorScheme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(2)].as_ref())
        .split(f.size());

    let items: Vec<ListItem> = state
        .entries
        .iter()
        .map(|path| {
            let file_name = path
                .file_name()
                .map_or_else(|| "..".into(), |s| s.to_string_lossy());

            let display_name = if path.is_dir() {
                format!("{}/", file_name)
            } else {
                file_name.to_string()
            };

            let style = if path.is_dir() {
                Style::default().fg(theme.link)
            } else {
                Style::default().fg(theme.fg)
            };
            ListItem::new(Span::styled(display_name, style))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(state.current_path.to_string_lossy().to_string())
                .style(Style::default().fg(theme.fg).bg(theme.bg)),
        )
        .highlight_style(
            Style::default()
                .bg(theme.selection_bg)
                .fg(theme.selection_fg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, chunks[0], &mut state.list_state);

    let status_bar_style = Style::default().fg(theme.fg).bg(theme.bg);
    let status_text = if state.in_command_mode {
        format!(":{}", state.command_input)
    } else if let Some(err) = &state.error_message {
        err.clone()
    } else {
        "j/k or ↓/↑: Move | Enter: Open | h or Backspace: Up | :<command> Enter: Run".to_string()
    };
    let status_bar = Paragraph::new(status_text).style(if state.error_message.is_some() {
        status_bar_style.fg(Color::Red)
    } else {
        status_bar_style
    });

    f.render_widget(status_bar, chunks[1]);
}

fn ui_preview(f: &mut Frame, state: &mut PreviewState, theme: &ColorScheme) {
    // Create a layout with a main area and a footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0), // Main content
            Constraint::Length(1), // Footer
        ])
        .split(f.size());

    // Main content paragraph without a block/border
    let paragraph = Paragraph::new(state.content.clone())
        .style(Style::default().fg(theme.fg).bg(theme.bg))
        .wrap(Wrap { trim: false })
        .scroll((state.scroll, 0));
    f.render_widget(paragraph, chunks[0]);

    // Footer
    let footer_text = format!("{} | {} chars | Press 'q' to close", state.title, state.char_count);
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(theme.comment).bg(theme.bg))
        .alignment(Alignment::Right);
    f.render_widget(footer, chunks[1]);
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
fn render_markdown(markdown_input: &str, br_placeholder: &str, theme: &ColorScheme) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default().fg(theme.fg)];
    let mut list_stack: Vec<u64> = Vec::new();
    let mut table_alignments: Vec<MarkdownAlignment> = Vec::new();
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
                        let base_style = Style::default()
                                .add_modifier(Modifier::BOLD)
                                .fg(theme.heading);
                        let style = if level >= HeadingLevel::H3 {
                            base_style.add_modifier(Modifier::DIM)
                        } else {
                            base_style
                        };
                        style_stack.push(style);
                    }
                    Tag::BlockQuote => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        let style = Style::default().fg(theme.quote_fg);
                        current_spans.push(Span::styled("▎".to_string(), Style::default().fg(theme.quote_border)));
                        current_spans.push(Span::raw(" ".to_string()));
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
                        let border_style = Style::default().fg(theme.comment);
                        lines.push(Line::from(vec![
                            Span::styled("┌─── ".to_string(), border_style),
                            Span::styled(lang, Style::default().fg(Color::Yellow)),
                        ]));
                        style_stack.push(Style::default().bg(theme.code_bg));
                    }
                    Tag::Table(aligns) => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        table_alignments = aligns;
                    }
                    Tag::TableHead => {
                        in_table_header = true;
                    }
                    Tag::TableRow => {
                        current_spans
                            .push(Span::styled("│ ".to_string(), Style::default().fg(theme.comment)));
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
                            .push(Span::styled(marker, Style::default().fg(theme.comment)));
                    }
                    Tag::Emphasis => {
                        style_stack.push(current_style.add_modifier(Modifier::ITALIC));
                    }
                    Tag::Strong => {
                        style_stack.push(current_style.add_modifier(Modifier::BOLD));
                    }
                    Tag::Strikethrough => {
                        style_stack.push(current_style.add_modifier(Modifier::CROSSED_OUT));
                    }
                    Tag::Link { .. } => {
                        style_stack
                        .push(Style::default().fg(theme.link).add_modifier(Modifier::UNDERLINED));
                    }
                    _ => {}
                }
            }
            MarkdownEvent::End(tag) => {
                match tag {
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
                            Style::default().fg(theme.comment),
                        )));
                        lines.push(Line::default());
                        style_stack.pop();
                    }
                    TagEnd::Table => {
                        table_alignments.clear();
                        lines.push(Line::default());
                    }
                    TagEnd::TableHead => {
                        in_table_header = false;
                    }
                    TagEnd::TableRow => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                    }
                    TagEnd::TableCell => {
                        current_spans.push(Span::styled(" │ ".to_string(), Style::default().fg(theme.comment)));
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
                }
            }
            MarkdownEvent::Text(text) => {
                let style = *style_stack.last().unwrap_or(&Style::default());
                if in_code_block {
                    for line in text.lines() {
                        lines.push(Line::from(vec![
                            Span::styled("│ ".to_string(), Style::default().fg(theme.comment)),
                            Span::styled(line.to_string(), style.fg(theme.fg)),
                        ]));
                    }
                } else {
                    let final_style = if in_table_header {
                        style.add_modifier(Modifier::BOLD)
                    } else {
                        style
                    };

                    if !br_placeholder.is_empty() && text.contains(br_placeholder) {
                        let mut last_pos = 0;
                        while let Some(placeholder_pos) = text[last_pos..].find(br_placeholder) {
                            let absolute_pos = last_pos + placeholder_pos;
                            let before = &text[last_pos..absolute_pos];
                            if !before.is_empty() {
                                current_spans.push(Span::styled(before.to_string(), final_style));
                            }
                            if !current_spans.is_empty() {
                                lines.push(Line::from(std::mem::take(&mut current_spans)));
                            }
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
                current_spans.push(Span::styled(html.to_string(), Style::default().fg(theme.comment)));
            }
            MarkdownEvent::Code(text) => {
                let style = Style::default().fg(theme.fg).bg(theme.inline_code_bg);
                current_spans.push(Span::styled(format!(" {} ", text), style));
            }
            MarkdownEvent::HardBreak => {
                if !current_spans.is_empty() {
                    lines.push(Line::from(std::mem::take(&mut current_spans)));
                }
            }
            MarkdownEvent::SoftBreak => {
                current_spans.push(Span::raw(" ".to_string()));
            }
            MarkdownEvent::Rule => {
                if !current_spans.is_empty() {
                    lines.push(Line::from(std::mem::take(&mut current_spans)));
                }
                lines.push(Line::from(Span::styled(
                    "─".repeat(80),
                    Style::default().fg(theme.hr),
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

