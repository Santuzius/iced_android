//! A test app for every iced feature Celeste uses, written the way Celeste is: `iced::daemon`, `Task`, `Subscription`.
//!
//! Each page covers one group; the checklist in ../README.md says what to try on a phone.
use iced_android::{self as platform, Insets};

use iced::font::Weight;
use iced::futures::SinkExt;
use iced::keyboard::{self, key::Named, Key};
use iced::theme::Mode;
use iced::widget::{
    button, center, column, container, mouse_area, opaque, pick_list, rich_text, row, scrollable, span, stack, svg, text, text_editor, text_input, toggler, tooltip,
};
use iced::{clipboard, stream, system, time, window};
use iced::{Element, Fill, Font, Subscription, Task, Theme};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub fn run() -> iced::Result {
    let mut daemon = iced::daemon(App::new, App::update, App::view).title(App::title).theme(App::theme).subscription(App::subscription).default_font(platform::DEFAULT_FONT);

    for font in platform::fonts() {
        daemon = daemon.font(font);
    }

    daemon.run()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Runtime,
    Text,
    Widgets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeChoice {
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];
}

impl std::fmt::Display for ThemeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ThemeChoice::System => "Follow system",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        })
    }
}

#[derive(Debug, Clone)]
enum Message {
    WindowOpened(window::Id),
    WindowClosed,
    Page(Page),
    Back,
    // Runtime
    Tick,
    StartTask,
    TaskDone(Duration),
    WorkerReady(mpsc::Sender<()>),
    WorkerEvent(u32),
    PokeWorker,
    SystemTheme(Mode),
    Key(String),
    Insets(Insets),
    // Text
    Name(String),
    Password(String),
    Submitted,
    Editor(text_editor::Action),
    Copy,
    Paste,
    Pasted(Option<String>),
    // Widgets
    Theme(ThemeChoice),
    Toggled(bool),
    Tapped,
    ShowDialog(bool),
}

struct App {
    window: Option<window::Id>,
    page: Page,
    insets: Insets,
    ticks: u32,
    task_started: Option<Instant>,
    task_result: Option<Duration>,
    worker: Option<mpsc::Sender<()>>,
    worker_events: u32,
    system_theme: Mode,
    last_key: String,
    name: String,
    password: String,
    submitted: Option<String>,
    editor: text_editor::Content,
    pasted: Option<String>,
    theme_choice: ThemeChoice,
    toggled: bool,
    taps: u32,
    dialog: bool,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let app = Self {
            window: None,
            page: Page::Runtime,
            insets: Insets::default(),
            ticks: 0,
            task_started: None,
            task_result: None,
            worker: None,
            worker_events: 0,
            system_theme: Mode::None,
            last_key: String::new(),
            name: String::new(),
            password: String::new(),
            submitted: None,
            editor: text_editor::Content::with_text("Multi-line text.\nTry the soft keyboard, swipe typing and autocorrect."),
            pasted: None,
            theme_choice: ThemeChoice::System,
            toggled: false,
            taps: 0,
            dialog: false,
        };
        // Celeste is a daemon that opens its window on demand; on Android the window opens once the activity has a surface.
        let (_id, open) = window::open(window::Settings::default());

        (app, Task::batch([open.map(Message::WindowOpened), system::theme().map(Message::SystemTheme)]))
    }

    fn title(&self, _window: window::Id) -> String {
        "Iced runtime on Android".to_owned()
    }

    fn theme(&self, _window: window::Id) -> Theme {
        if self.is_dark() { Theme::Dark } else { Theme::Light }
    }

    fn is_dark(&self) -> bool {
        match self.theme_choice {
            ThemeChoice::System => self.system_theme == Mode::Dark,
            ThemeChoice::Light => false,
            ThemeChoice::Dark => true,
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        // Like Celeste's sync engine: a plain thread pushes events through a tokio channel, which a subscription forwards as messages.
        let worker = Subscription::run(|| {
            stream::channel(16, async move |mut output| {
                let (poke_tx, mut poke_rx) = mpsc::channel::<()>(16);
                let (event_tx, mut event_rx) = mpsc::channel::<u32>(16);
                let _ = output.send(Message::WorkerReady(poke_tx)).await;

                std::thread::spawn(move || {
                    let mut count = 0;

                    while poke_rx.blocking_recv().is_some() {
                        std::thread::sleep(Duration::from_millis(300));
                        count += 1;

                        if event_tx.blocking_send(count).is_err() {
                            break;
                        }
                    }
                });

                while let Some(count) = event_rx.recv().await {
                    let _ = output.send(Message::WorkerEvent(count)).await;
                }
            })
        });
        // Celeste's back key is Escape on desktop; Android's back button and gesture arrive as BrowserBack.
        let keys = keyboard::listen().filter_map(|event| match event {
            keyboard::Event::KeyPressed { key: Key::Named(Named::Escape | Named::BrowserBack), .. } => Some(Message::Back),
            keyboard::Event::KeyPressed { key, .. } => Some(Message::Key(format!("{key:?}"))),
            _ => None,
        });

        Subscription::batch([
            worker,
            time::every(Duration::from_secs(1)).map(|_| Message::Tick),
            system::theme_changes().map(Message::SystemTheme),
            window::close_events().map(|_| Message::WindowClosed),
            platform::insets().map(Message::Insets),
            keys,
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowOpened(id) => self.window = Some(id),
            Message::WindowClosed => return iced::exit(),
            Message::Page(page) => self.page = page,
            Message::Back => {
                // Back closes the innermost thing that is open, like Escape in Celeste; at the top level the app goes to the background.
                if self.dialog {
                    self.dialog = false;
                } else if self.page != Page::Runtime {
                    self.page = Page::Runtime;
                } else if cfg!(target_os = "android") {
                    platform::move_to_background();
                } else if let Some(id) = self.window {
                    return window::close(id);
                }
            }
            Message::Tick => self.ticks += 1,
            Message::StartTask => {
                let started = Instant::now();
                self.task_started = Some(started);
                self.task_result = None;

                return Task::perform(
                    async move {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        started.elapsed()
                    },
                    Message::TaskDone,
                );
            }
            Message::TaskDone(elapsed) => {
                self.task_started = None;
                self.task_result = Some(elapsed);
            }
            Message::WorkerReady(sender) => self.worker = Some(sender),
            Message::WorkerEvent(count) => self.worker_events = count,
            Message::PokeWorker => {
                if let Some(worker) = &self.worker {
                    let _ = worker.try_send(());
                }
            }
            Message::SystemTheme(mode) => {
                self.system_theme = mode;
                platform::set_system_bars_dark(self.is_dark());
            }
            Message::Key(key) => self.last_key = key,
            Message::Insets(insets) => {
                let keyboard_opened = insets.bottom > self.insets.bottom;
                self.insets = insets;

                // As Android's own apps do: keep the field being typed into visible above the keyboard.
                if keyboard_opened {
                    return platform::scroll_to_focused();
                }
            }
            Message::Name(name) => self.name = name,
            Message::Password(password) => self.password = password,
            Message::Submitted => self.submitted = Some(self.name.clone()),
            Message::Editor(action) => self.editor.perform(action),
            Message::Copy => return clipboard::write(self.name.clone()),
            Message::Paste => return clipboard::read().map(Message::Pasted),
            Message::Pasted(contents) => self.pasted = contents,
            Message::Theme(choice) => {
                self.theme_choice = choice;
                platform::set_system_bars_dark(self.is_dark());
            }
            Message::Toggled(on) => self.toggled = on,
            Message::Tapped => self.taps += 1,
            Message::ShowDialog(show) => self.dialog = show,
        }

        Task::none()
    }

    fn view(&self, _window: window::Id) -> Element<'_, Message> {
        let tab = |label, page| button(text(label).center().width(Fill)).width(Fill).padding(12).style(if self.page == page { button::primary } else { button::secondary }).on_press(Message::Page(page));
        let tabs = row![tab("Runtime", Page::Runtime), tab("Text", Page::Text), tab("Widgets", Page::Widgets)].spacing(8);

        let page = match self.page {
            Page::Runtime => self.runtime(),
            Page::Text => self.text(),
            Page::Widgets => self.widgets(),
        };

        let content = container(column![tabs, scrollable(container(page).padding([0, 4])).spacing(4).height(Fill)].spacing(12)).padding(12);
        // The window covers the whole screen on Android; the insets keep content clear of the system bars and the soft keyboard.
        let screen = container(content).padding(self.insets).width(Fill).height(Fill);

        if self.dialog {
            let dialog = container(column![text("A modal dialog").size(20), text("Back, or the button below, closes it."), button("Close").on_press(Message::ShowDialog(false))].spacing(12)).padding(20).max_width(320).style(container::rounded_box);
            // `opaque` stops taps from reaching the page underneath; tapping the dimmed area closes the dialog.
            let backdrop = mouse_area(center(opaque(dialog)).style(|_| container::background(iced::Color { a: 0.6, ..iced::Color::BLACK }))).on_press(Message::ShowDialog(false));

            stack![screen, opaque(backdrop)].into()
        } else {
            screen.into()
        }
    }

    fn runtime(&self) -> Element<'_, Message> {
        let task = match (self.task_started, self.task_result) {
            (Some(_), _) => "running…".to_owned(),
            (None, Some(elapsed)) => format!("done after {} ms", elapsed.as_millis()),
            (None, None) => "not started".to_owned(),
        };

        column![
            section("Runtime"),
            line("time::every ticks", self.ticks.to_string()),
            row![button("Start Task::perform").on_press(Message::StartTask), text(task)].spacing(12).align_y(iced::Center),
            row![button("Poke worker thread").on_press_maybe(self.worker.as_ref().map(|_| Message::PokeWorker)), text(format!("{} events", self.worker_events))].spacing(12).align_y(iced::Center),
            line("System theme", format!("{:?}", self.system_theme)),
            line("Last key", if self.last_key.is_empty() { "–".to_owned() } else { self.last_key.clone() }),
            line("Insets (top/right/bottom/left)", format!("{:.0} / {:.0} / {:.0} / {:.0}", self.insets.top, self.insets.right, self.insets.bottom, self.insets.left)),
            text("Back: closes the dialog, then returns to this page, then sends the app to the background.").size(14),
        ]
        .spacing(12)
        .into()
    }

    fn text(&self) -> Element<'_, Message> {
        column![
            section("Text input"),
            text_input("Name (Enter submits)", &self.name).on_input(Message::Name).on_submit(Message::Submitted).padding(10),
            text(match &self.submitted {
                Some(name) => format!("Submitted: {name}"),
                None => "Not submitted yet".to_owned(),
            }),
            text_input("Password", &self.password).on_input(Message::Password).secure(true).padding(10),
            text_editor(&self.editor).on_action(Message::Editor).height(140).padding(10),
            section("Clipboard"),
            row![button("Copy name").on_press(Message::Copy), button("Paste").on_press(Message::Paste)].spacing(12),
            text(format!("Pasted: {}", self.pasted.as_deref().unwrap_or("–"))),
            section("Fonts"),
            rich_text![span("Rich text: "), span("bold").font(Font { weight: Weight::Bold, ..platform::DEFAULT_FONT }), span(", "), span("coloured").color(iced::Color::from_rgb(0.2, 0.5, 0.9)), span(".")].on_link_click(iced::never),
            text("Symbols: ⚠ ✓ ✗ → ↻ … – “quotes”"),
            text("Emoji: ☁️ 📁 🔄 ✅ 👍🏽 🇩🇪 🧑‍💻"),
            // Required by the licence of the bundled emoji font (iced_android's `emoji` feature), which Android 15+ uses.
            text("Emoji on Android 15+: Twemoji by Twitter, CC-BY 4.0").size(12),
            text("Umlauts and more: äöüß ÄÖÜ é ñ Ω жш"),
        ]
        .spacing(12)
        .into()
    }

    fn widgets(&self) -> Element<'_, Message> {
        let icon = svg(svg::Handle::from_memory(CLOUD_SVG)).width(48).height(48).style(|theme: &Theme, _| svg::Style { color: Some(theme.palette().primary) });
        let list = (1..=30).fold(column![].spacing(4), |list, i| list.push(row![container(text(format!("Row {i}"))).padding(8).width(Fill).style(container::rounded_box), button("Tap").on_press(Message::Tapped)].spacing(8).align_y(iced::Center)));

        column![
            section("Widgets"),
            row![text("Theme"), pick_list(ThemeChoice::ALL, Some(self.theme_choice), Message::Theme)].spacing(12).align_y(iced::Center),
            toggler(self.toggled).label("Toggler").on_toggle(Message::Toggled),
            row![icon, tooltip(button("Long-press for tooltip").on_press(Message::Tapped), container(text("A tooltip")).padding(8).style(container::rounded_box), tooltip::Position::Bottom)].spacing(12).align_y(iced::Center),
            mouse_area(container(text(format!("mouse_area: tapped {} times", self.taps))).padding(12).width(Fill).style(container::bordered_box)).on_release(Message::Tapped),
            button("Open modal dialog").on_press(Message::ShowDialog(true)),
            section("SVG"),
            text("Tabler icons (as in Celeste), tinted by the theme, and a logo with gradients and transparency:"),
            row![tabler(icondata::TbCloudCheckOutline), tabler(icondata::TbRefreshOutline), tabler(icondata::TbAlertTriangleOutline), tabler(icondata::TbFolderOpenOutline), tabler(icondata::TbPlayerPauseFilled), tabler(icondata::TbSettingsOutline), svg(svg::Handle::from_memory(LOGO_SVG)).width(48).height(48)].spacing(12).align_y(iced::Center),
            section("Scrolling list"),
            text("Swipe quickly: the list keeps scrolling. Swipes may start on a row's button."),
            list,
        ]
        .spacing(12)
        .into()
    }
}

fn section(title: &str) -> Element<'_, Message> {
    text(title).size(20).font(Font { weight: Weight::Bold, ..platform::DEFAULT_FONT }).into()
}

fn line(label: &str, value: String) -> Element<'_, Message> {
    row![text(label).width(Fill), text(value)].spacing(12).into()
}

/// An icondata icon as an SVG document. Its paths draw in black; the svg widget's style recolours them.
fn tabler(icon: icondata::Icon) -> Element<'static, Message> {
    let attribute = |name: &str, value: Option<&str>| value.map(|value| format!(r#" {name}="{}""#, value.replace("currentColor", "black"))).unwrap_or_default();
    let document = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{}"{}{}{}{}{}>{}</svg>"#,
        icon.view_box.unwrap_or("0 0 24 24"),
        attribute("fill", Some(icon.fill.unwrap_or("currentColor"))),
        attribute("stroke", icon.stroke),
        attribute("stroke-width", icon.stroke_width),
        attribute("stroke-linecap", icon.stroke_linecap),
        attribute("stroke-linejoin", icon.stroke_linejoin),
        icon.data,
    );

    svg(svg::Handle::from_memory(document.into_bytes())).width(28).height(28).style(|theme: &Theme, _| svg::Style { color: Some(theme.palette().text) }).into()
}

/// Gradients, a gradient transform and transparency, the features of Celeste's logo.
const LOGO_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128"><defs><linearGradient id="sky" x1="0" y1="0" x2="0" y2="128" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#62a0ea"/><stop offset="1" stop-color="#1a5fb4"/></linearGradient><linearGradient id="glow" x1="0" y1="0" x2="1" y2="0" gradientTransform="rotate(45 0.5 0.5)"><stop offset="0" stop-color="#ffffff"/><stop offset="1" stop-color="#ffffff" stop-opacity="0"/></linearGradient></defs><rect x="8" y="8" width="112" height="112" rx="24" fill="url(#sky)"/><path d="M36 86c-9 0-16-7-16-15s7-15 16-15c2-9 10-16 20-16 8 0 15 5 18 12 2-1 4-1 6-1 10 0 18 8 18 18s-8 17-18 17z" fill="#ffffff" opacity="0.9"/><circle cx="44" cy="44" r="28" fill="url(#glow)" opacity="0.5"/></svg>"##;

const CLOUD_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6.657 18c-2.572 0-4.657-2.007-4.657-4.483s2.085-4.482 4.657-4.482c.393-1.762 1.794-3.2 3.675-3.773 1.88-.572 3.956-.193 5.444 1 1.488 1.19 2.162 3.007 1.77 4.769h.99c1.913 0 3.464 1.56 3.464 3.486 0 1.927-1.551 3.487-3.465 3.487h-11.878"/></svg>"#;
