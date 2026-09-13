use alloc::{
    format,
    string::ToString,
    vec::Vec,
};
use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker, with_timeout};
use esp_alloc::HEAP;
use mousefood::{EmbeddedBackend, EmbeddedBackendConfig, fonts::mono_10x20_atlas};
use ratatui::{
    Frame, Terminal,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Paragraph, Wrap},
};

use crate::{
    screen::Display,
    state::{Page, RERENDER, STATE, State, set_state},
    web_mon::{Service, Severity},
};

const DOT: &str = "● ";

pub fn render_ui(spawner: Spawner, display: &'static mut Display) {
    spawner.spawn(render(display).expect("render ui"));
}

#[embassy_executor::task]
pub async fn cycle_pages() -> ! {
    let mut ticker = Ticker::every(Duration::from_secs(5));

    loop {
        ticker.next().await;

        set_state(|s| {
            s.page = s.page.next();
        })
        .await;
    }
}

#[embassy_executor::task]
async fn render(display: &'static mut Display) -> ! {
    let backend_config = EmbeddedBackendConfig {
        font_regular: mono_10x20_atlas(),
        ..Default::default()
    };
    let be = EmbeddedBackend::new(display, backend_config);
    let mut terminal = Terminal::new(be).expect("terminal");

    loop {
        let _ = with_timeout(Duration::from_secs(1), RERENDER.wait()).await;
        let state = STATE.lock().await;

        terminal
            .draw(|frame| render_page(frame, &state))
            .expect("render");
    }
}

fn render_page(frame: &mut Frame, state: &State) {
    match state.page {
        Page::Overview => render_overview(frame, &state),
        Page::Detail(i) => render_detail(frame, &state, i),
        Page::Network => render_initialization(frame, &state),
    }
}

fn render_initialization(frame: &mut Frame, state: &State) {
    let block = new_block().title("Initialization");
    let ip = match state.ip {
        None => "Acquiring IP...".to_string(),
        Some(ip) => format!("{}", ip.address()),
    };

    let mut body = Text::from(Line::from(ip));

    body.extend(Text::raw(format!("{}", HEAP.stats())));

    frame.render_widget(Paragraph::new(body).block(block), frame.area());
}

fn render_overview(frame: &mut Frame, state: &State) {
    let mut rows: Vec<Line<'_>> = Vec::new();

    for service in &Service::ALL {
        let (color, right) = match &state.service_statuses[service.index()] {
            Some(st) => (
                indicator_color(&st.indicator),
                if st.is_healthy() {
                    format!("{}s", st.fetched_at.elapsed().as_secs())
                } else if st.incidents.is_empty() {
                    st.indicator.to_string()
                } else {
                    format!(
                        "{} inc | {}s",
                        st.incidents.len(),
                        st.fetched_at.elapsed().as_secs()
                    )
                },
            ),
            None => (Color::DarkGray, "...".to_string()),
        };

        // 30 columns inside the border: dot (2) + name (left) + status (right).
        let name = service.to_string();
        let pad = 28usize.saturating_sub(name.chars().count() + right.chars().count());

        rows.push(Line::from(alloc::vec![
            Span::styled(DOT, Style::default().fg(color)),
            Span::from(name),
            Span::from(" ".repeat(pad)),
            Span::styled(right, Style::default().fg(Color::DarkGray)),
        ]));
    }

    let b = new_block()
        .title_top("Services")
        .title_top(Line::from("Last check").right_aligned());

    frame.render_widget(Paragraph::new(Text::from(rows)).block(b), frame.area());
}

fn new_block() -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::LightMagenta))
}

fn render_detail(frame: &mut Frame, state: &State, idx: usize) {
    let service = Service::ALL[idx];
    let title = service.to_string();

    let (body, footer) = match &state.service_statuses[idx] {
        None => (Text::from("no data yet"), None),
        Some(st) => {
            let mut lines: Vec<Line<'_>> = alloc::vec![Line::from(alloc::vec![
                Span::styled(DOT, Style::default().fg(indicator_color(&st.indicator))),
                Span::from(st.description.clone()),
            ])];

            for inc in &st.incidents {
                lines.push(Line::from(Span::styled(
                    inc.name.clone(),
                    Style::default().fg(indicator_color(&inc.impact)),
                )));
                lines.push(Line::from(Span::styled(
                    format!("  [{}]", inc.status),
                    Style::default().fg(Color::DarkGray),
                )));
                if let Some(latest) = inc.incident_updates.first() {
                    lines.push(Line::from(latest.body.clone()));
                }
            }

            (
                Text::from(lines),
                Some(Line::from(Span::styled(
                    format!("Checked {}s ago", st.fetched_at.elapsed().as_secs()),
                    Style::default().fg(Color::DarkGray),
                ))),
            )
        }
    };

    let mut block = new_block().title(title);

    if let Some(footer) = footer {
        block = block.title_bottom(footer.right_aligned());
    }

    frame.render_widget(
        Paragraph::new(body).block(block).wrap(Wrap { trim: true }),
        frame.area(),
    );
}

fn indicator_color(severity: &Severity) -> Color {
    match severity {
        Severity::None => Color::Green,
        Severity::Minor => Color::Yellow,
        Severity::Major => Color::LightRed,
        Severity::Critical => Color::Red,
        _ => Color::DarkGray,
    }
}
