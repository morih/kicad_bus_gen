//! ratatui TUI
//!
//! カラム: Ref | Pin Prefix | Prefix | Start | End | Wire(mm)
//! Pin Prefix: %d を含むフォーマット文字列（例: A%d, A_%d, A_{%d}）
//! 方向はパーサーが自動判定するためユーザー入力不要

use crate::parser::Schematic;
use crate::session::SavedRow;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction as LayoutDir, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};
use std::io;

// ────────────────────────────────────────────
// 公開型
// ────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct BusSpec {
    pub reference: String,
    pub pin_prefix: String, // フォーマット文字列 e.g. "A_{%d}"
    pub prefix: String,     // ネットラベル用プレフィックス e.g. "A"
    pub start: i32,
    pub end: i32,
    pub wire_len: f64,
}

// ────────────────────────────────────────────
// 内部型
// ────────────────────────────────────────────

const COLS: &[&str] = &["Ref", "Pin Prefix", "Prefix", "Start", "End", "Wire(inch)"];
const COL_WIDTHS: &[u16] = &[8, 16, 8, 6, 6, 10];

#[derive(Clone, Debug)]
struct RowData {
    reference: String,
    pin_prefix: String,
    prefix: String,
    start: String,
    end: String,
    wire_len: String,
}

impl Default for RowData {
    fn default() -> Self {
        Self {
            reference: String::new(),
            pin_prefix: String::new(),
            prefix: String::new(),
            start: "0".into(),
            end: "15".into(),
            wire_len: "0.2".into(),
        }
    }
}

impl RowData {
    fn from_saved(s: &SavedRow) -> Self {
        // wire_lenはセッションにmm単位で保存されているのでinchに変換して表示
        let wire_inch = s.wire_len / 25.4;
        Self {
            reference: s.reference.clone(),
            pin_prefix: s.pin_prefix.clone(),
            prefix: s.prefix.clone(),
            start: s.start.to_string(),
            end: s.end.to_string(),
            wire_len: format!("{:.4}", wire_inch),
        }
    }

    fn to_bus_spec(&self) -> Option<BusSpec> {
        let wire_inch: f64 = self.wire_len.parse().ok()?;
        Some(BusSpec {
            reference: self.reference.clone(),
            pin_prefix: self.pin_prefix.clone(),
            prefix: self.prefix.clone(),
            start: self.start.parse().ok()?,
            end: self.end.parse().ok()?,
            wire_len: wire_inch * 25.4, // inch → mm
        })
    }

    fn get_col(&self, col: usize) -> &str {
        match col {
            0 => &self.reference,
            1 => &self.pin_prefix,
            2 => &self.prefix,
            3 => &self.start,
            4 => &self.end,
            5 => &self.wire_len,
            _ => "",
        }
    }

    fn set_col(&mut self, col: usize, val: String) {
        match col {
            0 => self.reference = val,
            1 => self.pin_prefix = val,
            2 => self.prefix = val,
            3 => self.start = val,
            4 => self.end = val,
            5 => self.wire_len = val,
            _ => {}
        }
    }
}

// ────────────────────────────────────────────
// アプリ状態
// ────────────────────────────────────────────

struct App<'a> {
    schematic: &'a Schematic,
    rows: Vec<RowData>,
    sel_row: usize,
    sel_col: usize,
    editing: bool,
    edit_buf: String,
    table_state: TableState,
    status: String,
    generate: bool,
}

impl<'a> App<'a> {
    fn new(schematic: &'a Schematic, saved: Vec<SavedRow>) -> Self {
        let rows = if saved.is_empty() {
            vec![RowData::default()]
        } else {
            saved.iter().map(RowData::from_saved).collect()
        };
        let mut ts = TableState::default();
        ts.select(Some(0));
        Self {
            schematic,
            rows,
            sel_row: 0,
            sel_col: 0,
            editing: false,
            edit_buf: String::new(),
            table_state: ts,
            status: Self::help_status(),
            generate: false,
        }
    }

    fn help_status() -> String {
        "Pin Prefix format: A%d  A_%d  A_{%d} | ←→:列移動 ↑↓:行移動 Tab:次列 a:追加 d:削除 g:生成 q:終了".into()
    }

    fn next_col(&mut self) {
        self.sel_col = (self.sel_col + 1) % COLS.len();
    }
    fn prev_col(&mut self) {
        self.sel_col = if self.sel_col == 0 {
            COLS.len() - 1
        } else {
            self.sel_col - 1
        };
    }

    fn suggestions(&self) -> Vec<String> {
        let r = &self.rows[self.sel_row];
        match self.sel_col {
            0 => self.schematic.references(),
            1 => self.schematic.pin_prefixes_for(&r.reference),
            _ => vec![],
        }
    }

    /// Pin Prefix 確定後にピン向きをプレビューして Status に表示する
    fn refresh_status(&mut self) {
        let r = &self.rows[self.sel_row];

        if r.reference.is_empty() || r.pin_prefix.is_empty() {
            self.status = Self::help_status();
            return;
        }
        if !r.pin_prefix.contains("%d") {
            self.status =
                "Pin Prefix must contain %d  e.g.: A%d  A_%d  A_{%d} | Tab:次列 a:追加 d:削除 g:生成 q:終了"
                .into();
            return;
        }

        let start: i32 = r.start.parse().unwrap_or(0);
        // %d を start 番号に置換して先頭ピン名を作る
        let first_pin = Schematic::first_pin_name(&r.pin_prefix, start);

        match self.schematic.pin_out_direction(&r.reference, &first_pin) {
            Some(d) => {
                // format! に pin_prefix ({} 含む可能性) を渡さず文字列結合で構築
                self.status = "Pin direction: ".to_string()
                    + &format!("{:?}", d)
                    + "  (自動判定)  例: "
                    + &r.pin_prefix
                    + " => "
                    + &first_pin
                    + " | Tab:次列 a:追加 d:削除 g:生成 q:終了";
            }
            None => {
                self.status = "Warning: pin '".to_string()
                    + &first_pin
                    + "' not found on '"
                    + &r.reference
                    + "'  format例: A%d  A_%d  A_{%d} | Tab:次列 a:追加 d:削除 g:生成 q:終了";
            }
        }
    }

    fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for (i, r) in self.rows.iter().enumerate() {
            let n = i + 1;
            if r.reference.is_empty() {
                errors.push(format!("Row {n}: Ref empty"));
            }
            if r.pin_prefix.is_empty() {
                errors.push(format!("Row {n}: Pin Prefix empty"));
            }
            if r.prefix.is_empty() {
                errors.push(format!("Row {n}: Prefix empty"));
            }
            if r.start.parse::<i32>().is_err() {
                errors.push(format!("Row {n}: Start invalid"));
            }
            if r.end.parse::<i32>().is_err() {
                errors.push(format!("Row {n}: End invalid"));
            }
            if r.wire_len.parse::<f64>().is_err() {
                errors.push(format!("Row {n}: Wire len invalid"));
            }

            if !r.pin_prefix.is_empty() {
                if !r.pin_prefix.contains("%d") {
                    errors.push(format!("Row {n}: Pin Prefix must contain %d"));
                } else {
                    let start = r.start.parse().unwrap_or(0);
                    let end = r.end.parse().unwrap_or(0);
                    if let Err(e) =
                        self.schematic
                            .resolve_pin_names(&r.reference, &r.pin_prefix, start, end)
                    {
                        errors.push(format!("Row {n}: {e}"));
                    }
                }
            }
        }
        errors
    }
}

// ────────────────────────────────────────────
// TUI エントリポイント
// ────────────────────────────────────────────

pub fn run_tui(schematic: &Schematic, saved: Vec<SavedRow>) -> Result<Vec<BusSpec>, io::Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;
    let mut app = App::new(schematic, saved);

    loop {
        term.draw(|f| draw(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            if app.editing {
                match key.code {
                    KeyCode::Esc => {
                        app.editing = false;
                        app.edit_buf.clear();
                        app.status = App::help_status();
                    }
                    KeyCode::Enter | KeyCode::Tab => {
                        let val = app.edit_buf.clone();
                        app.rows[app.sel_row].set_col(app.sel_col, val);
                        app.editing = false;
                        app.edit_buf.clear();
                        app.next_col();
                        app.refresh_status();
                    }
                    KeyCode::Backspace => {
                        app.edit_buf.pop();
                    }
                    KeyCode::Char(c) => {
                        app.edit_buf.push(c);
                    }
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        app.generate = false;
                        break;
                    }
                    KeyCode::Char('g') => {
                        let errors = app.validate();
                        if errors.is_empty() {
                            app.generate = true;
                            break;
                        } else {
                            app.status = errors.join(" | ");
                        }
                    }
                    KeyCode::Char('a') => {
                        app.rows.push(RowData::default());
                        app.sel_row = app.rows.len() - 1;
                        app.sel_col = 0;
                        app.table_state.select(Some(app.sel_row));
                        app.status = App::help_status();
                    }
                    KeyCode::Char('d') => {
                        if app.rows.len() > 1 {
                            app.rows.remove(app.sel_row);
                            if app.sel_row >= app.rows.len() {
                                app.sel_row = app.rows.len() - 1;
                            }
                            app.table_state.select(Some(app.sel_row));
                            app.refresh_status();
                        }
                    }
                    KeyCode::Up => {
                        if app.sel_row > 0 {
                            app.sel_row -= 1;
                            app.table_state.select(Some(app.sel_row));
                            app.refresh_status();
                        }
                    }
                    KeyCode::Down => {
                        if app.sel_row + 1 < app.rows.len() {
                            app.sel_row += 1;
                            app.table_state.select(Some(app.sel_row));
                            app.refresh_status();
                        }
                    }
                    KeyCode::Tab => {
                        if key.modifiers.contains(KeyModifiers::SHIFT) {
                            app.prev_col();
                        } else {
                            app.next_col();
                        }
                    }
                    KeyCode::Right => {
                        app.next_col();
                    }
                    KeyCode::Left => {
                        app.prev_col();
                    }
                    KeyCode::Enter | KeyCode::Char(_) => {
                        app.editing = true;
                        app.edit_buf = app.rows[app.sel_row].get_col(app.sel_col).to_string();
                        if let KeyCode::Char(c) = key.code {
                            app.edit_buf = c.to_string();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        term.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    term.show_cursor()?;

    if !app.generate {
        return Ok(vec![]);
    }
    Ok(app.rows.iter().filter_map(|r| r.to_bus_spec()).collect())
}

// ────────────────────────────────────────────
// 描画
// ────────────────────────────────────────────

fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(area);
    draw_table(f, app, chunks[0]);
    draw_suggestions(f, app, chunks[1]);
    draw_status(f, app, chunks[2]);
}

fn draw_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header = Row::new(COLS.iter().map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    }))
    .height(1);

    let rows: Vec<Row> = app
        .rows
        .iter()
        .enumerate()
        .map(|(ri, r)| {
            let cells: Vec<Cell> = (0..COLS.len())
                .map(|ci| {
                    let is_sel = ri == app.sel_row && ci == app.sel_col;
                    let text = if is_sel && app.editing {
                        app.edit_buf.clone() + "|"
                    } else {
                        r.get_col(ci).to_string()
                    };
                    let style = if is_sel {
                        Style::default()
                            .bg(Color::Blue)
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    Cell::from(text).style(style)
                })
                .collect();
            Row::new(cells).height(1)
        })
        .collect();

    let widths: Vec<Constraint> = COL_WIDTHS.iter().map(|&w| Constraint::Length(w)).collect();
    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" KiCad Bus Generator (方向自動判定) "),
    );
    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_suggestions(f: &mut Frame, app: &mut App, area: Rect) {
    let suggs = app.suggestions();
    let filter = if app.editing {
        app.edit_buf.to_lowercase()
    } else {
        String::new()
    };
    let matched: Vec<&String> = suggs
        .iter()
        .filter(|s| s.to_lowercase().contains(&filter))
        .take(8)
        .collect();
    let line = if matched.is_empty() {
        Line::from(Span::raw(""))
    } else {
        Line::from(
            matched
                .iter()
                .map(|s| Span::styled(format!("[{}] ", s), Style::default().fg(Color::Cyan)))
                .collect::<Vec<_>>(),
        )
    };
    f.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Suggestions "),
        ),
        area,
    );
}

fn draw_status(f: &mut Frame, app: &mut App, area: Rect) {
    f.render_widget(
        Paragraph::new(app.status.clone())
            .block(Block::default().borders(Borders::ALL).title(" Status ")),
        area,
    );
}
