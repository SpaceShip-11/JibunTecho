use chrono::{Datelike, Duration as Dur, Local, NaiveDate};
use ratatui::{prelude::*, widgets::*};
use serde::{Deserialize, Serialize};
use crossterm::event::{
    self, DisableFocusChange, EnableFocusChange, Event as CEvent, KeyCode, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use std::{
    fs, io,
    path::PathBuf,
    time::{Duration, Instant},
};
// use crossterm::event::{self, Event as CEvent, KeyCode, KeyEventKind, KeyModifiers};
// use std::{fs, io, path::PathBuf, time::Duration};

const TABS: [&str; 6] = ["Year", "Month", "Week", "Day", "Life", "Notes"];
const DOW: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

// ───────────────────────── data ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Event, // timed / all-day schedule entry
    Todo,  // checkbox for a day
    Diary, // daily log line
    Goal,  // "Life" section: goals / bucket list
    Note,  // free notes page
}
impl Kind {
    fn order(self) -> u8 {
        match self {
            Kind::Event => 0,
            Kind::Todo => 1,
            Kind::Diary => 2,
            _ => 3,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct Item {
    kind: Kind,
    date: Option<String>, // YYYY-MM-DD
    time: Option<String>, // HH:MM
    title: String,
    done: bool,
}
impl Item {
    fn on(&self, d: NaiveDate) -> bool {
        self.date.as_deref() == Some(&d.format("%Y-%m-%d").to_string())
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Store {
    items: Vec<Item>,
}

fn data_path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
        });
    base.join("jibun/data.json")
}

impl Store {
    fn load() -> Self {
        fs::read_to_string(data_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    fn save(&self) {
        let p = data_path();
        if let Some(d) = p.parent() {
            let _ = fs::create_dir_all(d);
        }
        let _ = fs::write(p, serde_json::to_string_pretty(self).unwrap());
    }
}

// ───────────────────────── helpers ─────────────────────────

fn days_in_month(y: i32, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (NaiveDate::from_ymd_opt(ny, nm, 1).unwrap() - NaiveDate::from_ymd_opt(y, m, 1).unwrap())
        .num_days() as u32
}
fn month_start(d: NaiveDate) -> NaiveDate {
    d.with_day(1).unwrap()
}
fn add_months(d: NaiveDate, n: i32) -> NaiveDate {
    let total = d.year() * 12 + d.month0() as i32 + n;
    let y = total.div_euclid(12);
    let m = total.rem_euclid(12) as u32 + 1;
    NaiveDate::from_ymd_opt(y, m, d.day().min(days_in_month(y, m))).unwrap()
}
fn week_start(d: NaiveDate) -> NaiveDate {
    d - Dur::days(d.weekday().num_days_from_monday() as i64)
}

fn parse_time(s: &str) -> (Option<String>, String) {
    if let Some((a, b)) = s.split_once(' ') {
        let ok = a.len() == 5
            && a.as_bytes()[2] == b':'
            && a.chars().enumerate().all(|(i, c)| i == 2 || c.is_ascii_digit());
        if ok {
            return (Some(a.to_string()), b.trim().to_string());
        }
    }
    (None, s.trim().to_string())
}

fn label(i: &Item) -> String {
    let check = if i.done { "[x]" } else { "[ ]" };
    match i.kind {
        Kind::Event => format!("{} {}", i.time.as_deref().unwrap_or("--:--"), i.title),
        Kind::Todo | Kind::Goal => format!("{} {}", check, i.title),
        Kind::Diary => i.title.clone(),
        Kind::Note => format!("• {}", i.title),
    }
}

// ───────────────────────── app ─────────────────────────

struct App {
    store: Store,
    tab: usize,
    sel: NaiveDate,
    cursor: usize,
    input: Option<(Kind, String)>,
    quit: bool,
}

impl App {
    fn new() -> Self {
        Self {
            store: Store::load(),
            tab: 1,
            sel: Local::now().date_naive(),
            cursor: 0,
            input: None,
            quit: false,
        }
    }

    fn day_items(&self, d: NaiveDate) -> Vec<usize> {
        let mut v: Vec<usize> = self
            .store
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.on(d) && matches!(i.kind, Kind::Event | Kind::Todo | Kind::Diary))
            .map(|(n, _)| n)
            .collect();
        v.sort_by_key(|&n| {
            let i = &self.store.items[n];
            (i.kind.order(), i.time.clone().unwrap_or_default())
        });
        v
    }

    fn by_kind(&self, k: Kind) -> Vec<usize> {
        self.store
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.kind == k)
            .map(|(n, _)| n)
            .collect()
    }

    fn list_items(&self) -> Vec<usize> {
        match self.tab {
            3 => self.day_items(self.sel),
            4 => self.by_kind(Kind::Goal),
            5 => self.by_kind(Kind::Note),
            _ => vec![],
        }
    }

    fn has_items(&self, d: NaiveDate) -> bool {
        self.store.items.iter().any(|i| i.on(d))
    }

    fn shift(&mut self, dir: i32, big: bool) {
        let d = dir as i64;
        self.sel = match (self.tab, big) {
            (0, false) => add_months(self.sel, dir),
            (0, true) => add_months(self.sel, 12 * dir),
            (1, false) | (2, false) | (3, false) => self.sel + Dur::days(d),
            (1, true) => add_months(self.sel, dir),
            (2, true) | (3, true) => self.sel + Dur::days(7 * d),
            _ => self.sel,
        };
        self.cursor = 0;
    }

    fn vertical(&mut self, dir: i32) {
        match self.tab {
            0 => self.sel = add_months(self.sel, 4 * dir), // next row of 4 months
            1 | 2 => self.sel += Dur::days(7 * dir as i64),
            _ => {
                let n = self.list_items().len();
                if n > 0 {
                    self.cursor = if dir > 0 {
                        (self.cursor + 1).min(n - 1)
                    } else {
                        self.cursor.saturating_sub(1)
                    };
                }
            }
        }
    }

    fn start_input(&mut self, kind: Kind) {
        self.input = Some((kind, String::new()));
    }

    fn commit(&mut self) {
        if let Some((kind, buf)) = self.input.take() {
            let buf = buf.trim().to_string();
            if buf.is_empty() {
                return;
            }
            let (time, title) = if kind == Kind::Event {
                parse_time(&buf)
            } else {
                (None, buf)
            };
            let date = matches!(kind, Kind::Event | Kind::Todo | Kind::Diary)
                .then(|| self.sel.format("%Y-%m-%d").to_string());
            self.store.items.push(Item { kind, date, time, title, done: false });
            self.store.save();
        }
    }

    fn selected_id(&self) -> Option<usize> {
        self.list_items().get(self.cursor).copied()
    }

    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        // input mode
        if let Some((_, buf)) = self.input.as_mut() {
            match code {
                KeyCode::Enter => self.commit(),
                KeyCode::Esc => self.input = None,
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c) => buf.push(c),
                _ => {}
            }
            return;
        }

        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }

        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Tab => {
                self.tab = (self.tab + 1) % TABS.len();
                self.cursor = 0;
            }
            KeyCode::BackTab => {
                self.tab = (self.tab + TABS.len() - 1) % TABS.len();
                self.cursor = 0;
            }
            KeyCode::Char(c @ '1'..='6') => {
                self.tab = c as usize - '1' as usize;
                self.cursor = 0;
            }
            KeyCode::Char('g') => self.sel = Local::now().date_naive(),
            KeyCode::Char('h') | KeyCode::Left => self.shift(-1, false),
            KeyCode::Char('l') | KeyCode::Right => self.shift(1, false),
            KeyCode::Char('H') | KeyCode::Char('[') => self.shift(-1, true),
            KeyCode::Char('L') | KeyCode::Char(']') => self.shift(1, true),
            KeyCode::Char('j') | KeyCode::Down => self.vertical(1),
            KeyCode::Char('k') | KeyCode::Up => self.vertical(-1),
            KeyCode::Enter if self.tab < 3 => {
                self.tab = 3;
                self.cursor = 0;
            }
            KeyCode::Char('a') => match self.tab {
                4 => self.start_input(Kind::Goal),
                5 => self.start_input(Kind::Note),
                _ => self.start_input(Kind::Event),
            },
            KeyCode::Char('o') if self.tab < 4 => self.start_input(Kind::Todo),
            KeyCode::Char('n') if self.tab < 4 => self.start_input(Kind::Diary),
            KeyCode::Char(' ') | KeyCode::Char('x') => {
                if let Some(id) = self.selected_id() {
                    let it = &mut self.store.items[id];
                    if matches!(it.kind, Kind::Todo | Kind::Goal) {
                        it.done = !it.done;
                        self.store.save();
                    }
                }
            }
            KeyCode::Char('D') => {
                if let Some(id) = self.selected_id() {
                    self.store.items.remove(id);
                    self.store.save();
                    let n = self.list_items().len();
                    self.cursor = self.cursor.min(n.saturating_sub(1));
                }
            }
            _ => {}
        }
    }
}

// ───────────────────────── drawing ─────────────────────────

fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(f.area());

    let tabs = Tabs::new(TABS.iter().map(|t| Line::from(*t)))
        .select(app.tab)
        .block(
            Block::bordered()
                .title(format!(" Jibun Techo · {} ", app.sel.format("%Y-%m-%d %a"))),
        )
        .highlight_style(Style::new().bold().fg(Color::Yellow));
    f.render_widget(tabs, chunks[0]);

    match app.tab {
        0 => draw_year(f, chunks[1], app),
        1 => draw_month(f, chunks[1], app),
        2 => draw_week(f, chunks[1], app),
        3 => draw_day(f, chunks[1], app),
        4 => draw_list(f, chunks[1], app, "Life · goals / bucket list"),
        _ => draw_list(f, chunks[1], app, "Notes"),
    }

    let footer = match &app.input {
        Some((k, buf)) => {
            let what = match k {
                Kind::Event => "event (optional HH:MM first)",
                Kind::Todo => "todo",
                Kind::Diary => "diary",
                Kind::Goal => "goal",
                Kind::Note => "note",
            };
            Line::from(format!(" add {what}: {buf}█"))
        }
        None => Line::from(
            " Tab/1-6 tabs  h/l day  H/L bigger step  j/k move  g today  Enter open day  a event/new  o todo  n diary  space done  D del  q quit",
        )
        .style(Style::new().add_modifier(Modifier::DIM)),
    };
    f.render_widget(Paragraph::new(footer), chunks[2]);
}

fn mini_month(f: &mut Frame, area: Rect, app: &App, y: i32, m: u32) {
    let today = Local::now().date_naive();
    let first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    let offset = first.weekday().num_days_from_monday();
    let mut lines = vec![Line::from("Mo Tu We Th Fr Sa Su")
        .style(Style::new().add_modifier(Modifier::DIM))];
    let mut spans: Vec<Span> = (0..offset).map(|_| Span::raw("   ")).collect();
    for day in 1..=days_in_month(y, m) {
        let d = NaiveDate::from_ymd_opt(y, m, day).unwrap();
        let mut st = Style::new();
        if app.has_items(d) {
            st = st.fg(Color::Cyan).add_modifier(Modifier::BOLD);
        }
        if d == today {
            st = st.fg(Color::Yellow).add_modifier(Modifier::BOLD);
        }
        if d == app.sel {
            st = st.add_modifier(Modifier::REVERSED);
        }
        spans.push(Span::styled(format!("{:>2} ", day), st));
        if (offset + day) % 7 == 0 {
            lines.push(Line::from(std::mem::take(&mut spans)));
        }
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    let border = if m == app.sel.month() && y == app.sel.year() {
        Style::new().fg(Color::Magenta)
    } else {
        Style::new()
    };
    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_style(border)
                .title(first.format(" %B ").to_string()),
        ),
        area,
    );
}

fn draw_year(f: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Ratio(1, 3); 3]).split(area);
    for r in 0..3 {
        let cols = Layout::horizontal([Constraint::Ratio(1, 4); 4]).split(rows[r]);
        for c in 0..4 {
            mini_month(f, cols[c], app, app.sel.year(), (r * 4 + c + 1) as u32);
        }
    }
}

fn draw_month(f: &mut Frame, area: Rect, app: &App) {
    let today = Local::now().date_naive();
    let first = month_start(app.sel);
    let offset = first.weekday().num_days_from_monday() as i64;
    let dim = days_in_month(first.year(), first.month()) as i64;
    let weeks = ((offset + dim + 6) / 7) as usize;

    let rows = Layout::vertical(
        std::iter::once(Constraint::Length(1))
            .chain((0..weeks).map(|_| Constraint::Ratio(1, weeks as u32))),
    )
    .split(area);

    let hc = Layout::horizontal([Constraint::Ratio(1, 7); 7]).split(rows[0]);
    for (i, n) in DOW.iter().enumerate() {
        f.render_widget(
            Paragraph::new(*n)
                .alignment(Alignment::Center)
                .style(Style::new().add_modifier(Modifier::BOLD)),
            hc[i],
        );
    }

    let start = first - Dur::days(offset);
    for w in 0..weeks {
        let cells = Layout::horizontal([Constraint::Ratio(1, 7); 7]).split(rows[w + 1]);
        for c in 0..7 {
            let d = start + Dur::days((w * 7 + c) as i64);
            let mut border = Style::new();
            if d.month() != first.month() {
                border = border.fg(Color::DarkGray);
            }
            if d == today {
                border = border.fg(Color::Yellow);
            }
            if d == app.sel {
                border = border.fg(Color::Magenta).add_modifier(Modifier::BOLD);
            }
            let h = cells[c].height.saturating_sub(2) as usize;
            let lines: Vec<Line> = app
                .day_items(d)
                .iter()
                .filter(|&&n| app.store.items[n].kind != Kind::Diary)
                .take(h)
                .map(|&n| Line::from(label(&app.store.items[n])))
                .collect();
            f.render_widget(
                Paragraph::new(lines).block(
                    Block::bordered()
                        .border_style(border)
                        .title(format!("{}", d.day())),
                ),
                cells[c],
            );
        }
    }
}

fn draw_week(f: &mut Frame, area: Rect, app: &App) {
    let today = Local::now().date_naive();
    let ws = week_start(app.sel);
    let cols = Layout::horizontal([Constraint::Ratio(1, 7); 7]).split(area);
    for c in 0..7 {
        let d = ws + Dur::days(c as i64);
        let mut border = Style::new();
        if d == today {
            border = border.fg(Color::Yellow);
        }
        if d == app.sel {
            border = border.fg(Color::Magenta).add_modifier(Modifier::BOLD);
        }
        let lines: Vec<Line> = app
            .day_items(d)
            .iter()
            .map(|&n| {
                let it = &app.store.items[n];
                let mut st = Style::new();
                if it.done {
                    st = st.add_modifier(Modifier::CROSSED_OUT);
                }
                if it.kind == Kind::Diary {
                    st = st.add_modifier(Modifier::DIM);
                }
                Line::styled(label(it), st)
            })
            .collect();
        f.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }).block(
                Block::bordered()
                    .border_style(border)
                    .title(format!(" {} {} ", DOW[c], d.day())),
            ),
            cols[c],
        );
    }
}

fn pane<'a>(app: &App, kind: Kind, title: &'a str, ids: &[usize]) -> Paragraph<'a> {
    let mut lines = vec![];
    for (pos, &n) in ids.iter().enumerate() {
        let it = &app.store.items[n];
        if it.kind != kind {
            continue;
        }
        let mut st = Style::new();
        if pos == app.cursor {
            st = st.add_modifier(Modifier::REVERSED);
        }
        if it.done {
            st = st.add_modifier(Modifier::CROSSED_OUT);
        }
        lines.push(Line::styled(label(it), st));
    }
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::bordered().title(title))
}

fn draw_day(f: &mut Frame, area: Rect, app: &App) {
    let ids = app.day_items(app.sel);
    let cols = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(area);
    let left = Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).split(cols[0]);
    f.render_widget(pane(app, Kind::Event, " Schedule ", &ids), left[0]);
    f.render_widget(pane(app, Kind::Todo, " To-do ", &ids), left[1]);
    f.render_widget(pane(app, Kind::Diary, " Diary ", &ids), cols[1]);
}

fn draw_list(f: &mut Frame, area: Rect, app: &App, title: &str) {
    let ids = app.list_items();
    let lines: Vec<Line> = ids
        .iter()
        .enumerate()
        .map(|(pos, &n)| {
            let it = &app.store.items[n];
            let mut st = Style::new();
            if pos == app.cursor {
                st = st.add_modifier(Modifier::REVERSED);
            }
            if it.done {
                st = st.add_modifier(Modifier::CROSSED_OUT);
            }
            Line::styled(label(it), st)
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(format!(" {title} "))),
        area,
    );
}

// ───────────────────────── waybar status ─────────────────────────

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn status() {
    let store = Store::load();
    let today = Local::now().date_naive();
    let mut tip = String::new();
    let mut pending = 0;

    for (name, d) in [("Today", today), ("Tomorrow", today + Dur::days(1))] {
        tip.push_str(&format!("<b>{} · {}</b>\n", name, d.format("%a %b %d")));
        let mut its: Vec<&Item> = store
            .items
            .iter()
            .filter(|i| i.on(d) && i.kind != Kind::Diary)
            .collect();
        its.sort_by_key(|i| (i.kind.order(), i.time.clone().unwrap_or_default()));
        if its.is_empty() {
            tip.push_str("  —\n");
        }
        for i in its {
            if d == today && !i.done {
                pending += 1;
            }
            tip.push_str(&format!("  {}\n", esc(&label(i))));
        }
        tip.push('\n');
    }

    let mut text = format!("\u{f00ed} {}", today.format("%a %d %b"));
    if pending > 0 {
        text.push_str(&format!("  {pending}"));
    }
    let out = serde_json::json!({
        "text": text,
        "tooltip": tip.trim_end(),
        "class": if pending > 0 { "pending" } else { "clear" },
    });
    println!("{out}");
}

// ───────────────────────── main ─────────────────────────

// fn main() -> io::Result<()> {
//     if std::env::args().nth(1).as_deref() == Some("status") {
//         status();
//         return Ok(());
//     }
//     let mut term = ratatui::init();
//     let mut app = App::new();
//     let res = (|| -> io::Result<()> {
//         while !app.quit {
//             term.draw(|f| draw(f, &app))?;
//             if event::poll(Duration::from_millis(250))? {
//                 if let CEvent::Key(k) = event::read()? {
//                     if k.kind == KeyEventKind::Press {
//                         app.on_key(k.code, k.modifiers);
//                     }
//                 }
//             }
//         }
//         Ok(())
//     })();
//     ratatui::restore();
//     res
// }

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("status") {
        status();
        return Ok(());
    }
    // --stay keeps the planner open when it loses focus (handy for `cargo run`)
    let autoclose = !args.iter().any(|a| a == "--stay");

    let mut term = ratatui::init();
    if autoclose {
        let _ = execute!(io::stdout(), EnableFocusChange);
    }
    let started = Instant::now();
    let mut app = App::new();

    let res = (|| -> io::Result<()> {
        while !app.quit {
            term.draw(|f| draw(f, &app))?;
            if event::poll(Duration::from_millis(250))? {
                match event::read()? {
                    CEvent::Key(k) if k.kind == KeyEventKind::Press => {
                        app.on_key(k.code, k.modifiers)
                    }
                    // ignore focus flicker while the window is still opening
                    CEvent::FocusLost
                        if autoclose && started.elapsed() > Duration::from_millis(500) =>
                    {
                        app.quit = true
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })();

    if autoclose {
        let _ = execute!(io::stdout(), DisableFocusChange);
    }
    ratatui::restore();
    res
}