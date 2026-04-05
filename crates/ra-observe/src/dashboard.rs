use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Tabs},
    Frame, Terminal,
};

use crate::metrics_collector::{DashboardData, LogEventType, MetricsCollector};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DashboardTab {
    Overview,
    EventLog,
}

struct DashboardState {
    tab: DashboardTab,
    log_scroll: usize,
    selected_agent: usize,
}

impl DashboardState {
    fn new() -> Self {
        Self {
            tab: DashboardTab::Overview,
            log_scroll: 0,
            selected_agent: 0,
        }
    }
}

pub async fn run_dashboard(collector: Arc<MetricsCollector>) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = DashboardState::new();

    loop {
        let data = collector.snapshot_async().await;

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3), // header + tabs
                    Constraint::Min(6),    // main content
                    Constraint::Length(1), // footer
                ])
                .split(f.area());

            render_header(f, chunks[0], &data, &state);

            match state.tab {
                DashboardTab::Overview => render_overview(f, chunks[1], &data, &state),
                DashboardTab::EventLog => render_event_log(f, chunks[1], &data, &state),
            }

            render_footer(f, chunks[2], &state);
        })?;

        if event::poll(Duration::from_millis(300))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Tab => {
                        state.tab = match state.tab {
                            DashboardTab::Overview => DashboardTab::EventLog,
                            DashboardTab::EventLog => DashboardTab::Overview,
                        };
                    }
                    KeyCode::Char('1') => state.tab = DashboardTab::Overview,
                    KeyCode::Char('2') => state.tab = DashboardTab::EventLog,
                    KeyCode::Char('j') | KeyCode::Down => match state.tab {
                        DashboardTab::EventLog => {
                            if state.log_scroll + 1 < data.recent_events.len() {
                                state.log_scroll += 1;
                            }
                        }
                        DashboardTab::Overview => {
                            if !data.agents.is_empty()
                                && state.selected_agent + 1 < data.agents.len()
                            {
                                state.selected_agent += 1;
                            }
                        }
                    },
                    KeyCode::Char('k') | KeyCode::Up => match state.tab {
                        DashboardTab::EventLog => {
                            state.log_scroll = state.log_scroll.saturating_sub(1);
                        }
                        DashboardTab::Overview => {
                            state.selected_agent = state.selected_agent.saturating_sub(1);
                        }
                    },
                    KeyCode::Char('G') => {
                        // Jump to end
                        if state.tab == DashboardTab::EventLog {
                            state.log_scroll = data.recent_events.len().saturating_sub(1);
                        }
                    }
                    KeyCode::Char('g') => {
                        // Jump to top
                        state.log_scroll = 0;
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn render_header(f: &mut Frame, area: Rect, data: &DashboardData, state: &DashboardState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(30)])
        .split(area);

    // Stats
    let stats = Paragraph::new(Line::from(vec![
        Span::styled(
            " RA ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("Agents: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(
                "{}/{}",
                data.workflow.completed_agents + data.workflow.failed_agents,
                data.workflow.total_agents
            ),
            Style::default().fg(Color::White),
        ),
        Span::raw("  "),
        Span::styled("Cost: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("${:.4}", data.workflow.total_cost_usd),
            Style::default().fg(Color::Green),
        ),
        Span::raw("  "),
        Span::styled("Tokens: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}k", data.workflow.total_tokens() / 1000),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled("Events: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", data.recent_events.len()),
            Style::default().fg(Color::Magenta),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(stats, chunks[0]);

    // Tabs
    let tab_titles = vec![
        Span::styled(
            " 1:Overview ",
            if state.tab == DashboardTab::Overview {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
        Span::styled(
            " 2:Events ",
            if state.tab == DashboardTab::EventLog {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
    ];

    let tabs = Tabs::new(tab_titles.into_iter().map(Line::from).collect::<Vec<_>>())
        .select(match state.tab {
            DashboardTab::Overview => 0,
            DashboardTab::EventLog => 1,
        })
        .block(Block::default().borders(Borders::ALL).title("View"))
        .highlight_style(Style::default().fg(Color::Cyan));
    f.render_widget(tabs, chunks[1]);
}

fn render_overview(f: &mut Frame, area: Rect, data: &DashboardData, state: &DashboardState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // Agent table
    let header_row = Row::new(vec![
        Cell::from(" # ").style(Style::default().fg(Color::DarkGray)),
        Cell::from("Agent ID").style(Style::default().fg(Color::Yellow)),
        Cell::from("In Tokens").style(Style::default().fg(Color::Yellow)),
        Cell::from("Out Tokens").style(Style::default().fg(Color::Yellow)),
        Cell::from("Cost").style(Style::default().fg(Color::Yellow)),
        Cell::from("Duration").style(Style::default().fg(Color::Yellow)),
        Cell::from("Turns").style(Style::default().fg(Color::Yellow)),
        Cell::from("Retries").style(Style::default().fg(Color::Yellow)),
    ]);

    let rows: Vec<Row> = data
        .agents
        .iter()
        .enumerate()
        .map(|(i, (id, m))| {
            let style = if i == state.selected_agent {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(format!(" {} ", i + 1)),
                Cell::from(id.to_string()[..8].to_string()),
                Cell::from(format!("{}", m.input_tokens)),
                Cell::from(format!("{}", m.output_tokens)),
                Cell::from(format!("${:.4}", m.total_cost_usd)),
                Cell::from(format!("{}ms", m.duration_ms)),
                Cell::from(format!("{}", m.turns)),
                Cell::from(if m.retries > 0 {
                    format!("{}", m.retries)
                } else {
                    "-".to_string()
                }),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Length(10),
            Constraint::Length(11),
            Constraint::Length(11),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(7),
            Constraint::Length(8),
        ],
    )
    .header(header_row)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Agents")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(table, chunks[0]);

    // Mini event log (last N events that fit)
    render_log_pane(f, chunks[1], data, 0, "Recent Events (Tab for full view)");
}

fn render_event_log(f: &mut Frame, area: Rect, data: &DashboardData, state: &DashboardState) {
    render_log_pane(
        f,
        area,
        data,
        state.log_scroll,
        &format!(
            "Event Log ({} events, scroll: j/k, top: g, end: G)",
            data.recent_events.len()
        ),
    );
}

fn render_log_pane(f: &mut Frame, area: Rect, data: &DashboardData, scroll: usize, title: &str) {
    let visible_height = area.height.saturating_sub(2) as usize; // borders

    let items: Vec<ListItem> = data
        .recent_events
        .iter()
        .rev()
        .skip(scroll)
        .take(visible_height)
        .map(|entry| {
            let (color, bold) = match entry.event_type {
                LogEventType::StateChange => (Color::Green, false),
                LogEventType::Assistant => (Color::Cyan, false),
                LogEventType::Result => (Color::White, true),
                LogEventType::Error => (Color::Red, true),
                LogEventType::RateLimit => (Color::Yellow, true),
                LogEventType::System => (Color::DarkGray, false),
            };

            let mut style = Style::default().fg(color);
            if bold {
                style = style.add_modifier(Modifier::BOLD);
            }

            let time = entry.timestamp.format("%H:%M:%S");
            let id_short = &entry.agent_id.to_string()[..6];
            let label = entry.event_type.label();

            // Truncate message to fit
            let max_msg_len = area.width.saturating_sub(30) as usize;
            let msg: String = entry.message.chars().take(max_msg_len).collect();

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", time), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("[{}] ", id_short), Style::default().fg(Color::Blue)),
                Span::styled(format!("{:<5} ", label), style),
                Span::styled(msg, style),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Magenta)),
    );
    f.render_widget(list, area);
}

fn render_footer(f: &mut Frame, area: Rect, _state: &DashboardState) {
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" q ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
        Span::raw(" quit  "),
        Span::styled(
            " Tab ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
        Span::raw(" switch view  "),
        Span::styled(
            " j/k ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
        Span::raw(" scroll  "),
        Span::styled(
            " g/G ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
        Span::raw(" top/end"),
    ]));
    f.render_widget(footer, area);
}
