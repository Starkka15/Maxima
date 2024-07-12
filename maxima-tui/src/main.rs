use clap::{Parser, Subcommand};

use futures::StreamExt;
use inquire::Select;
use lazy_static::lazy_static;
use log::{debug, error, info, warn};
use regex::Regex;
use service::{BridgeThread, MaximaLibRequest, MaximaLibResponse};

use std::{io::stdout, sync::Arc, time::{Duration, Instant}};

#[cfg(windows)]
use is_elevated::is_elevated;

#[cfg(windows)]
use maxima::{
    core::background_service::request_registry_setup,
    util::service::{is_service_running, is_service_valid, register_service_user, start_service},
};

use maxima::{
    content::downloader::ZipDownloader,
    core::{
        auth::{nucleus_token_exchange, TokenResponse},
        clients::JUNO_PC_CLIENT_ID,
        launch::LaunchMode,
        library::OwnedTitle,
        service_layer::{
            ServiceGetBasicPlayerRequestBuilder, ServiceGetLegacyCatalogDefsRequestBuilder,
            ServiceLegacyOffer, ServicePlayer, SERVICE_REQUEST_GETBASICPLAYER,
            SERVICE_REQUEST_GETLEGACYCATALOGDEFS,
        },
        LockedMaxima, MaximaOptionsBuilder,
    },
    ooa,
    rtm::client::BasicPresence,
};
use maxima::{
    content::ContentService,
    core::{
        auth::{
            context::AuthContext,
            login::{begin_oauth_login_flow, manual_login},
            nucleus_auth_exchange,
        },
        launch,
        service_layer::ServiceUserGameProduct,
        Maxima, MaximaEvent,
    },
    util::{log::init_logger, native::take_foreground_focus, registry::check_registry_validity},
};

lazy_static! {
    static ref MANUAL_LOGIN_PATTERN: Regex = Regex::new(r"^(.*):(.*)$").unwrap();
}

use anyhow::{bail, Result};
use color_eyre::config::HookBuilder;
use ratatui::{
    crossterm::{
        event::{self, Event, KeyCode, KeyEventKind},
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        ExecutableCommand,
    },
    prelude::*,
    style::palette::tailwind,
    widgets::*,
};
use strum::{Display, EnumIter, FromRepr, IntoEnumIterator};

mod service;

struct App {
    state: AppState,
    selected_tab: SelectedTab,
    popup: Option<String>,
    bridge: BridgeThread,
    username: String,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum AppState {
    #[default]
    Running,
    Quitting
}

#[derive(Default, Clone, Copy, Display, FromRepr, EnumIter)]
enum SelectedTab {
    #[default]
    #[strum(to_string = "Games")]
    Games,
    #[strum(to_string = "Downloads")]
    Downloads,
    #[strum(to_string = "Settings")]
    Settings,
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::Running,
            selected_tab: SelectedTab::Games,
            popup: Some("Logging in...".to_owned()),
            bridge: BridgeThread::new(),
            username: String::new(),
        }
    }

    fn run(&mut self, terminal: &mut Terminal<impl Backend>) -> Result<()> {
        self.bridge.tx.send(MaximaLibRequest::LoginRequest).unwrap();

        while self.state == AppState::Running {
            self.draw(terminal)?;
            self.handle_events()?;
            self.handle_responses()?;
        }

        Ok(())
    }

    fn draw(&self, terminal: &mut Terminal<impl Backend>) -> Result<()> {
        terminal.draw(|frame| frame.render_widget(self, frame.size()))?;
        Ok(())
    }

    fn handle_events(&mut self) -> std::io::Result<()> {
        if !event::poll(Duration::from_millis(100))? {
            return Ok(());
        }

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                use KeyCode::*;
                match key.code {
                    Char('l') | Right => self.next_tab(),
                    Char('h') | Left => self.previous_tab(),
                    Char('q') | Esc => self.quit(),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn handle_responses(&mut self) -> std::io::Result<()> {
        let message = self.bridge.rx.try_recv();
        if message.is_err() {
            return Ok(());
        }

        let message = message.unwrap();
        match message {
            MaximaLibResponse::LoginResponse(response) => {
                self.popup = None;
                self.username = response.name;
            }
            MaximaLibResponse::LoginCacheEmpty => {
                self.popup = Some("No login cache found".to_owned());
            }
            MaximaLibResponse::InteractionThreadDiedResponse => {
                self.popup = Some("Interaction thread died".to_owned());
            }
            _ => {}
        };

        Ok(())
    }

    pub fn next_tab(&mut self) {
        self.selected_tab = self.selected_tab.next();
    }

    pub fn previous_tab(&mut self) {
        self.selected_tab = self.selected_tab.previous();
    }

    pub fn quit(&mut self) {
        self.state = AppState::Quitting;
    }
}

impl SelectedTab {
    /// Get the previous tab, if there is no previous tab return the current tab.
    fn previous(self) -> Self {
        let current_index: usize = self as usize;
        let previous_index = current_index.saturating_sub(1);
        Self::from_repr(previous_index).unwrap_or(self)
    }

    /// Get the next tab, if there is no next tab return the current tab.
    fn next(self) -> Self {
        let current_index = self as usize;
        let next_index = current_index.saturating_add(1);
        Self::from_repr(next_index).unwrap_or(self)
    }
}

/// helper function to create a centered rect using up certain percentage of the available rect `r`
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use Constraint::*;
        let vertical = Layout::vertical([Length(1), Min(0), Length(1)]);
        let [header_area, inner_area, footer_area] = vertical.areas(area);

        let title_text = if self.username.is_empty() {
            "Maxima".to_string()
        } else {
            format!("Maxima - {}", self.username)
        };

        let horizontal = Layout::horizontal([Min(0), Length(title_text.len() as u16)]);
        let [tabs_area, title_area] = horizontal.areas(header_area);

        render_title(title_area, buf, &title_text);
        if !self.username.is_empty() {
            self.render_tabs(tabs_area, buf);
            self.selected_tab.render(inner_area, buf);
            render_footer(footer_area, buf);
        }

        if let Some(popup) = &self.popup {
            let popup_area = centered_rect(20, 20, area);
            Paragraph::new(popup.as_str())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(tailwind::SLATE.c700),
                )
                .wrap(Wrap { trim: true })
                .render(popup_area, buf);
        }
    }
}

impl App {
    fn render_tabs(&self, area: Rect, buf: &mut Buffer) {
        let titles = SelectedTab::iter().map(SelectedTab::title);
        let highlight_style = (Color::default(), self.selected_tab.palette().c700);
        let selected_tab_index = self.selected_tab as usize;
        Tabs::new(titles)
            .highlight_style(highlight_style)
            .select(selected_tab_index)
            .padding("", "")
            .divider(" ")
            .render(area, buf);
    }
}

fn render_title(area: Rect, buf: &mut Buffer, text: &str) {
    text.bold().render(area, buf);
}

fn render_footer(area: Rect, buf: &mut Buffer) {
    Line::raw("◄ ► to change tab | Press q to quit")
        .centered()
        .render(area, buf);
}

impl Widget for SelectedTab {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // in a real app these might be separate widgets
        match self {
            Self::Games => self.render_games(area, buf),
            Self::Downloads => self.render_tab1(area, buf),
            Self::Settings => self.render_tab2(area, buf),
        }
    }
}

impl SelectedTab {
    /// Return tab's name as a styled `Line`
    fn title(self) -> Line<'static> {
        format!("  {self}  ")
            .fg(tailwind::SLATE.c200)
            .bg(self.palette().c900)
            .into()
    }

    fn render_games(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("Hello, World!")
            .block(self.block())
            .render(area, buf);
    }

    fn render_tab1(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("Welcome to the Ratatui tabs example!")
            .block(self.block())
            .render(area, buf);
    }

    fn render_tab2(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("Look! I'm different than others!")
            .block(self.block())
            .render(area, buf);
    }

    /// A block surrounding the tab's content
    fn block(self) -> Block<'static> {
        Block::bordered()
            .border_set(symbols::border::PROPORTIONAL_TALL)
            .padding(Padding::horizontal(1))
            .border_style(self.palette().c700)
    }

    const fn palette(self) -> tailwind::Palette {
        match self {
            Self::Games => tailwind::BLUE,
            Self::Downloads => tailwind::EMERALD,
            Self::Settings => tailwind::INDIGO,
        }
    }
}

#[tokio::main]
async fn main() {
    let result = startup().await;

    if let Some(e) = result.err() {
        match std::env::var("RUST_BACKTRACE") {
            Ok(_) => error!("{}:\n{}", e, e.backtrace().to_string()),
            Err(_) => error!("{}", e),
        }
    }
}

#[cfg(windows)]
async fn native_setup() -> Result<()> {
    if !is_elevated() {
        if !is_service_valid()? {
            info!("Installing service...");
            register_service_user()?;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        if !is_service_running()? {
            info!("Starting service...");
            start_service().await?;
        }
    }

    if let Err(err) = check_registry_validity() {
        warn!("{}, fixing...", err);
        request_registry_setup().await?;
    }

    Ok(())
}

#[cfg(not(windows))]
async fn native_setup() -> Result<()> {
    use maxima::util::registry::set_up_registry;

    if let Err(err) = check_registry_validity() {
        warn!("{}, fixing...", err);
        set_up_registry()?;
    }

    Ok(())
}

async fn startup() -> Result<()> {
    //init_logger();

    info!("Starting Maxima...");

    native_setup().await?;

    // Take back the focus since the browser and bootstrap will take it
    take_foreground_focus()?;

    init_error_hooks().unwrap();
    let mut terminal = init_terminal().unwrap();
    App::new().run(&mut terminal)?;
    restore_terminal().unwrap();
    Ok(())
}

fn init_error_hooks() -> color_eyre::Result<()> {
    let (panic, error) = HookBuilder::default().into_hooks();
    let panic = panic.into_panic_hook();
    let error = error.into_eyre_hook();
    color_eyre::eyre::set_hook(Box::new(move |e| {
        let _ = restore_terminal();
        error(e)
    }))?;
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        panic(info);
    }));
    Ok(())
}

fn init_terminal() -> color_eyre::Result<Terminal<impl Backend>> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal() -> color_eyre::Result<()> {
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

async fn start_game(
    offer_id: &str,
    game_path_override: Option<String>,
    game_args: Vec<String>,
    login: Option<String>,
    maxima_arc: LockedMaxima,
) -> Result<()> {
    {
        let mut maxima = maxima_arc.lock().await;
        maxima.start_lsx(maxima_arc.clone()).await?;

        if login.is_none() {
            maxima.rtm().login().await?;
        }
    }

    if login.is_none() {
        launch::start_game(
            maxima_arc.clone(),
            LaunchMode::Online(offer_id.to_owned()),
            game_path_override,
            game_args,
        )
        .await?;
    } else if let Some(captures) = MANUAL_LOGIN_PATTERN.captures(&login.unwrap()) {
        let persona = &captures[1];
        let password = &captures[2];

        launch::start_game(
            maxima_arc.clone(),
            LaunchMode::OnlineOffline(offer_id.to_owned(), persona.to_owned(), password.to_owned()),
            game_path_override,
            game_args,
        )
        .await?;
    }

    loop {
        let mut maxima = maxima_arc.lock().await;

        for event in maxima.consume_pending_events() {
            match event {
                MaximaEvent::ReceivedLSXRequest(_pid, _request) => (),
                MaximaEvent::Unknown => todo!(),
            }
        }

        maxima.update_playing_status();
        if maxima.playing().is_none() {
            break;
        }

        drop(maxima);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    Ok(())
}
