use crate::context_menu;
use crate::editor::EditorBuffer;
use crate::filesystem::{
    build_tree, expand_directory, read_text_file, refresh_tree, save_text_file, supported_file,
    visible_nodes, FileNode, WorkspaceWatcher, SUPPORTED_FILE_EXTENSIONS,
};
use crate::markdown::{is_markdown_file, parse_items};
use crate::syntax;
use iced::alignment::{Horizontal, Vertical};
use iced::highlighter;
use iced::keyboard::{self, Key, Modifiers};
use iced::widget::{button, column, container, mouse_area, opaque, row, scrollable, stack, text, text_editor, tooltip, Space};
use iced::{
    window, Background, Border, Color, Element, Fill, Font, Length, Shadow, Subscription, Task,
    Theme, Vector,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const MENU_BUTTON_WIDTH: f32 = 52.0;
const MENU_GAP: f32 = 4.0;
const BRAND_BLOCK_WIDTH: f32 = 122.0;
const TITLEBAR_INSET_X: f32 = 12.0;
const TITLEBAR_INSET_Y: f32 = 8.0;
const LAYOUT_GAP: f32 = 6.0;
const SIDEBAR_WIDTH: f32 = 286.0;
const PREVIEW_WIDTH: f32 = 388.0;

#[derive(Debug, Clone)]
enum Message {
    OpenFile,
    OpenFolder,
    Save,
    Undo,
    Redo,
    TogglePreview,
    TogglePreviewFullscreen,
    ToggleTheme,
    ToggleContextMenu,
    ShowAbout,
    ShowShortcuts,
    ToggleMenu(MenuKind),
    MinimizeWindow,
    ToggleMaximizeWindow,
    CloseWindow,
    EditorAction(text_editor::Action),
    TreePressed(PathBuf),
    LinkClicked(String),
    OpenAboutRepo,
    ScrollActivity(Instant),
    PollWatcher,
    UiPulse,
    MenuHoverChanged(HoverRegion, bool),
    CloseMenuIfUnhovered,
    KeyboardEvent(keyboard::Event),
    DismissModal,
    OpenAnyway,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeMode {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    File,
    View,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowControlKind {
    Minimize,
    Maximize,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionTone {
    Sidebar,
    Editor,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoverRegion {
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct MenuRegionState {
    overlay_hovered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ModalState {
    UnsupportedFile { message: String, path: PathBuf },
    About,
}

struct App {
    editor: EditorBuffer,
    tree: Option<FileNode>,
    workspace_root: Option<PathBuf>,
    watcher: Option<WorkspaceWatcher>,
    markdown_items: Vec<iced::widget::markdown::Item>,
    preview_open: bool,
    preview_fullscreen: bool,
    theme_mode: ThemeMode,
    status: String,
    last_tree_click: Option<(PathBuf, Instant)>,
    scrollbar_flash_until: Option<Instant>,
    context_menu_registered: bool,
    modal: Option<ModalState>,
    open_menu: Option<MenuKind>,
    menu_regions: MenuRegionState,
    menu_close_deadline: Option<Instant>,
}

pub fn run(initial_path: Option<PathBuf>) -> iced::Result {
    let icon = window::icon::from_file_data(
        include_bytes!("../../icon.png"),
        None,
    )
    .ok();

    iced::application(move || App::new(initial_path.clone()), App::update, App::view)
        .theme(App::theme)
        .subscription(App::subscription)
        .window(window::Settings {
            icon,
            decorations: false,
            ..Default::default()
        })
        .run()
}

impl App {
    fn new(initial_path: Option<PathBuf>) -> (Self, Task<Message>) {
        let mut app = Self {
            editor: EditorBuffer::new(None, String::new()),
            tree: None,
            workspace_root: None,
            watcher: None,
            markdown_items: Vec::new(),
            preview_open: true,
            preview_fullscreen: false,
            theme_mode: ThemeMode::Light,
            status: "Open a file or folder to start editing.".to_string(),
            last_tree_click: None,
            scrollbar_flash_until: None,
            context_menu_registered: context_menu::is_registered(),
            modal: None,
            open_menu: None,
            menu_regions: MenuRegionState::default(),
            menu_close_deadline: None,
        };

        if let Some(path) = initial_path {
            if path.is_dir() {
                app.open_folder(path);
            } else if path.is_file() {
                app.open_file(path);
            }
        }

        (app, Task::none())
    }

    fn theme(&self) -> Theme {
        match self.theme_mode {
            ThemeMode::Light => Theme::Light,
            ThemeMode::Dark => Theme::TokyoNight,
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![keyboard::listen().map(Message::KeyboardEvent)];

        if self.watcher.is_some() {
            subscriptions.push(
                iced::time::every(Duration::from_millis(700)).map(|_| Message::PollWatcher),
            );
        }

        if self.scrollbars_are_visible() {
            subscriptions
                .push(iced::time::every(Duration::from_millis(120)).map(|_| Message::UiPulse));
        }

        if self.open_menu.is_some() {
            subscriptions.push(
                iced::time::every(Duration::from_millis(60)).map(|_| Message::CloseMenuIfUnhovered),
            );
        }

        Subscription::batch(subscriptions)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenFile => {
                self.open_menu = None;

                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Text and code", SUPPORTED_FILE_EXTENSIONS)
                    .pick_file()
                {
                    self.open_file(path);
                }

                Task::none()
            }
            Message::OpenFolder => {
                self.open_menu = None;

                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.open_folder(path);
                }

                Task::none()
            }
            Message::Save => {
                self.open_menu = None;
                self.save_active_file();
                Task::none()
            }
            Message::Undo => {
                if self.editor.undo() {
                    self.refresh_markdown();
                    self.status = "Undid last edit.".to_string();
                } else {
                    self.status = "Nothing to undo.".to_string();
                }

                Task::none()
            }
            Message::Redo => {
                if self.editor.redo() {
                    self.refresh_markdown();
                    self.status = "Redid last edit.".to_string();
                } else {
                    self.status = "Nothing to redo.".to_string();
                }

                Task::none()
            }
            Message::TogglePreview => {
                self.open_menu = None;
                self.preview_open = !self.preview_open;
                if !self.preview_open {
                    self.preview_fullscreen = false;
                }
                self.status = if self.preview_open {
                    "Markdown preview enabled.".to_string()
                } else {
                    "Markdown preview hidden.".to_string()
                };
                Task::none()
            }
            Message::TogglePreviewFullscreen => {
                self.open_menu = None;
                self.preview_fullscreen = !self.preview_fullscreen;
                if self.preview_fullscreen {
                    self.preview_open = true;
                }
                self.status = if self.preview_fullscreen {
                    "Preview expanded to full width.".to_string()
                } else {
                    "Preview restored to side panel.".to_string()
                };
                Task::none()
            }
            Message::ToggleTheme => {
                self.open_menu = None;
                self.theme_mode = match self.theme_mode {
                    ThemeMode::Light => ThemeMode::Dark,
                    ThemeMode::Dark => ThemeMode::Light,
                };
                self.status = match self.theme_mode {
                    ThemeMode::Light => "Light theme active.".to_string(),
                    ThemeMode::Dark => "Dark theme active.".to_string(),
                };
                Task::none()
            }
            Message::ToggleContextMenu => {
                self.open_menu = None;
                if self.context_menu_registered {
                    match context_menu::unregister() {
                        Ok(()) => {
                            self.context_menu_registered = false;
                            self.status = "Removed MyPad from the context menu.".to_string();
                        }
                        Err(error) => {
                            self.status = format!("Failed to update context menu: {error}");
                        }
                    }
                } else {
                    match context_menu::current_exe_path().and_then(|exe| context_menu::register(&exe)) {
                        Ok(()) => {
                            self.context_menu_registered = true;
                            self.status = "Added MyPad to the context menu.".to_string();
                        }
                        Err(error) => {
                            self.status = format!("Failed to update context menu: {error}");
                        }
                    }
                }
                Task::none()
            }
            Message::ShowAbout => {
                self.open_menu = None;
                self.modal = Some(ModalState::About);
                Task::none()
            }
            Message::ShowShortcuts => {
                self.open_menu = None;
                self.status =
                    "Shortcuts: Ctrl+S saves, Ctrl+Z undoes, Ctrl+Shift+Z redoes, and Tab indents. Double-click a sidebar file to open it."
                        .to_string();
                Task::none()
            }
            Message::ToggleMenu(kind) => {
                self.open_menu = toggle_menu(self.open_menu, kind);
                self.menu_close_deadline = None;

                if self.open_menu.is_some() {
                    self.menu_regions = MenuRegionState {
                        overlay_hovered: true,
                    };
                } else {
                    self.menu_regions = MenuRegionState::default();
                }

                Task::none()
            }
            Message::MinimizeWindow => {
                self.open_menu = None;
                window::latest().and_then(|id| window::minimize(id, true))
            }
            Message::ToggleMaximizeWindow => {
                self.open_menu = None;
                window::latest().and_then(window::toggle_maximize)
            }
            Message::CloseWindow => {
                self.open_menu = None;
                window::latest().and_then(window::close)
            }
            Message::EditorAction(action) => {
                self.editor.apply_action(action);
                self.refresh_markdown();
                Task::none()
            }
            Message::TreePressed(path) => {
                self.open_menu = None;
                self.handle_tree_press(path);
                Task::none()
            }
            Message::LinkClicked(link) => {
                self.open_menu = None;
                self.modal = None;

                if let Err(error) = open::that(link.as_str()) {
                    self.status = format!("Failed to open link: {error}");
                } else {
                    self.status = format!("Opened link: {link}");
                }

                Task::none()
            }
            Message::OpenAboutRepo => {
                self.open_menu = None;
                self.modal = None;

                let repo = about_repo_url();

                if let Err(error) = open::that(repo) {
                    self.status = format!("Failed to open link: {error}");
                } else {
                    self.status = format!("Opened link: {repo}");
                }

                Task::none()
            }
            Message::ScrollActivity(at) => {
                self.scrollbar_flash_until = Some(at + Duration::from_millis(900));
                Task::none()
            }
            Message::PollWatcher => {
                self.poll_watcher();
                Task::none()
            }
            Message::UiPulse => {
                if self
                    .scrollbar_flash_until
                    .is_some_and(|until| until <= Instant::now())
                {
                    self.scrollbar_flash_until = None;
                }

                Task::none()
            }
            Message::MenuHoverChanged(region, hovered) => {
                self.menu_regions = next_menu_hover_state(self.menu_regions, region, hovered);
                self.menu_close_deadline = menu_close_deadline(Instant::now(), self.menu_regions);
                Task::none()
            }
            Message::CloseMenuIfUnhovered => {
                if self.open_menu.is_some()
                    && self
                        .menu_close_deadline
                        .is_some_and(|deadline| deadline <= Instant::now())
                    && menu_should_close(self.menu_regions)
                {
                    self.open_menu = None;
                    self.menu_regions = MenuRegionState::default();
                    self.menu_close_deadline = None;
                }

                Task::none()
            }
            Message::KeyboardEvent(event) => {
                if is_save_shortcut(&event) {
                    self.save_active_file();
                }

                if is_escape_shortcut(&event) {
                    self.open_menu = None;
                    self.modal = None;
                }

                Task::none()
            }
            Message::DismissModal => {
                self.modal = None;
                Task::none()
            }
            Message::OpenAnyway => {
                if let Some(ModalState::UnsupportedFile { path, .. }) = self.modal.take() {
                    self.open_file(path);
                }
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let palette = self.palette();
        let chrome = self.chrome_view(palette);

        let mut content_row = row![].spacing(content_section_gap()).height(Fill);
        let show_preview = self.preview_open && is_markdown_file(self.editor.path());

        if !self.preview_fullscreen {
            if let Some(tree) = &self.tree {
                content_row = content_row.push(self.sidebar_view(tree));
            }
        }

        if !self.preview_fullscreen {
            content_row = content_row.push(self.editor_view());
        }

        if show_preview {
            content_row = content_row.push(self.preview_view());
        }

        let layout = column![chrome, content_row]
            .spacing(LAYOUT_GAP)
            .padding([10, 12])
            .height(Fill)
            .width(Fill);

        let base = container(layout)
            .width(Fill)
            .height(Fill)
            .style(root_style(palette));

        let layered: Element<'_, Message> = if let Some(menu) = self.open_menu {
            stack([base.into(), opaque(self.menu_overlay(menu, palette))])
                .width(Fill)
                .height(Fill)
                .into()
        } else {
            base.into()
        };

        if let Some(modal) = &self.modal {
            stack([
                layered,
                opaque(self.modal_overlay(modal, palette)),
            ])
            .width(Fill)
            .height(Fill)
            .into()
        } else {
            layered
        }
    }

    fn chrome_view(&self, palette: Palette) -> Element<'_, Message> {
        container(self.titlebar_view(palette))
            .width(Fill)
            .style(titlebar_shell_style(palette))
            .into()
    }

    fn modal_overlay(&self, modal: &ModalState, palette: Palette) -> Element<'_, Message> {
        let card = match modal {
            ModalState::UnsupportedFile { message, .. } => container(
                column![
                    text("Unsupported File")
                        .font(Self::display_font())
                        .size(16)
                        .color(palette.text_strong),
                    text(message.clone())
                        .font(Self::body_font())
                        .size(13)
                        .color(palette.text_muted),
                    container(
                        row![
                            icon_button(
                                palette,
                                open_anyway_icon(),
                                Message::OpenAnyway,
                                false,
                                "Open anyway",
                            ),
                            icon_button(
                                palette,
                                dismiss_icon(),
                                Message::DismissModal,
                                true,
                                "Dismiss",
                            ),
                        ]
                        .spacing(8),
                    )
                    .width(Fill)
                    .align_x(Horizontal::Right),
                ]
                .spacing(10)
                .width(280),
            )
            .padding([18, 22])
            .style(menu_panel_style(palette)),
            ModalState::About => container(
                column![
                    row![
                        info_pill(palette, "RUST EDITOR", true),
                        info_pill(palette, "LIGHTWEIGHT", false),
                    ]
                    .spacing(6),
                    column![
                        text("About MyPad")
                            .font(Self::display_font())
                            .size(24)
                            .color(palette.text_strong),
                        text(about_body_message())
                            .font(Self::body_font())
                            .size(13)
                            .line_height(1.45)
                            .color(palette.text_muted),
                    ]
                    .spacing(8),
                    container(
                        column![
                            text("Repository")
                                .font(Self::mono_font())
                                .size(10)
                                .color(palette.text_soft),
                            text(about_repo_url())
                                .font(Self::mono_font())
                                .size(12)
                                .color(palette.accent_text),
                        ]
                        .spacing(4),
                    )
                    .padding([10, 12])
                    .width(Fill)
                    .style(modal_inline_panel_style(palette)),
                    container(
                        row![
                            icon_button(
                                palette,
                                dismiss_icon(),
                                Message::DismissModal,
                                false,
                                "Close",
                            ),
                            icon_button(
                                palette,
                                about_repo_icon(),
                                Message::OpenAboutRepo,
                                true,
                                "Open repository",
                            ),
                        ]
                        .spacing(8),
                    )
                    .width(Fill)
                    .align_x(Horizontal::Right),
                ]
                .spacing(14)
                .width(360),
            )
            .padding([20, 22])
            .style(modal_card_style(palette)),
        };

        mouse_area(
            container(card)
                .width(Fill)
                .height(Fill)
                .align_x(Horizontal::Center)
                .align_y(Vertical::Center)
                .style(move |_| container::Style {
                    text_color: None,
                    background: Some(Background::Color(with_alpha(Color::BLACK, 0.35))),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    snap: false,
                }),
        )
        .on_press(Message::DismissModal)
        .into()
    }

    fn menu_overlay(&self, kind: MenuKind, palette: Palette) -> Element<'_, Message> {
        let menu_column = column![
            Space::new().height(menu_overlay_offset_y()),
            self.menu_panel_host(kind, palette)
        ];

        let tracked = mouse_area(menu_column)
            .on_enter(Message::MenuHoverChanged(HoverRegion::Overlay, true))
            .on_exit(Message::MenuHoverChanged(HoverRegion::Overlay, false));

        container(tracked)
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn titlebar_view(&self, palette: Palette) -> Element<'_, Message> {
        let brand = container(
            row![
                container(
                    text("M")
                        .font(Self::display_font())
                        .size(16)
                        .color(Color::WHITE)
                )
                .padding([5, 9])
                .style(brand_mark_style(palette)),
                text("MyPad")
                    .font(Self::display_font())
                    .size(19)
                    .color(palette.text_strong),
            ]
            .spacing(8)
            .align_y(Vertical::Center),
        )
        .width(BRAND_BLOCK_WIDTH);

        let menus = row![
            menu_button(palette, MenuKind::File, self.open_menu == Some(MenuKind::File)),
            menu_button(palette, MenuKind::View, self.open_menu == Some(MenuKind::View)),
            menu_button(palette, MenuKind::Help, self.open_menu == Some(MenuKind::Help)),
        ]
        .spacing(MENU_GAP)
        .width(Length::Shrink);

        let title = text(window_title_label(&self.editor))
            .font(Self::body_font())
            .size(13)
            .width(Fill)
            .align_x(Horizontal::Center)
            .color(palette.text_muted);

        let controls = row![
            theme_toggle_button(palette, self.theme_mode),
            window_control_button(
                palette,
                WindowControlKind::Minimize,
                "—",
                Message::MinimizeWindow,
                "Minimize",
            ),
            window_control_button(
                palette,
                WindowControlKind::Maximize,
                "□",
                Message::ToggleMaximizeWindow,
                "Maximize",
            ),
            window_control_button(
                palette,
                WindowControlKind::Close,
                "×",
                Message::CloseWindow,
                "Close",
            ),
        ]
        .spacing(6)
        .align_y(Vertical::Center);

        container(
            row![row![brand, menus].spacing(12).align_y(Vertical::Center), title, controls]
                .align_y(Vertical::Center)
                .spacing(10),
        )
        .padding([TITLEBAR_INSET_Y, TITLEBAR_INSET_X])
        .style(titlebar_style(palette))
        .into()
    }

    fn menu_panel_host(&self, kind: MenuKind, palette: Palette) -> Element<'_, Message> {
        row![
            Space::new().width(menu_panel_offset(kind)),
            self.menu_panel(kind, palette),
            Space::new().width(Fill)
        ]
        .padding([4.0, TITLEBAR_INSET_X + BRAND_BLOCK_WIDTH + 12.0])
        .width(Fill)
        .into()
    }

    fn menu_panel(&self, kind: MenuKind, palette: Palette) -> Element<'_, Message> {
        let content = match kind {
            MenuKind::File => column![
                menu_item_button(palette, "Open File", Message::OpenFile),
                menu_item_button(palette, "Open Folder", Message::OpenFolder),
                separator(palette.divider),
                menu_item_button(palette, "Save", Message::Save),
            ]
            .spacing(4),
            MenuKind::View => column![
                menu_item_button(
                    palette,
                    if self.preview_open {
                        "Hide Markdown Preview"
                    } else {
                        "Show Markdown Preview"
                    },
                    Message::TogglePreview,
                ),
                menu_item_button(
                    palette,
                    if self.preview_fullscreen {
                        "Exit Full Preview"
                    } else {
                        "Full Preview"
                    },
                    Message::TogglePreviewFullscreen,
                ),
                menu_item_button(
                    palette,
                    if self.theme_mode == ThemeMode::Light {
                        "Switch To Dark Theme"
                    } else {
                        "Switch To Light Theme"
                    },
                    Message::ToggleTheme,
                ),
                separator(palette.divider),
                menu_item_button(
                    palette,
                    if self.context_menu_registered {
                        "Remove From Context Menu"
                    } else {
                        "Add To Context Menu"
                    },
                    Message::ToggleContextMenu,
                ),
            ]
            .spacing(4),
            MenuKind::Help => column![
                menu_item_button(palette, "About MyPad", Message::ShowAbout),
                menu_item_button(palette, "Keyboard Shortcuts", Message::ShowShortcuts),
                separator(palette.divider),
                menu_info_line(palette, "Ctrl+S saves the active file"),
                menu_info_line(palette, "Ctrl+Z undoes, Ctrl+Shift+Z redoes"),
                menu_info_line(palette, "Tab indents, Shift+Tab unindents"),
                menu_info_line(palette, "Double-click a file to open it"),
            ]
            .spacing(4),
        };

        container(content)
            .width(220)
            .padding([10, 10])
            .style(menu_panel_style(palette))
            .into()
    }

    fn scrollbars_are_visible(&self) -> bool {
        self.scrollbar_flash_until
            .is_some_and(|until| until > Instant::now())
    }

    fn open_file(&mut self, path: PathBuf) {
        match read_text_file(&path) {
            Ok(contents) => {
                self.editor.set_from_disk(Some(path.clone()), contents);
                self.status = format!("Opened {}", path.display());

                if self
                    .workspace_root
                    .as_ref()
                    .is_none_or(|root| !path.starts_with(root))
                {
                    self.tree = None;
                    self.workspace_root = None;
                    self.watcher = WorkspaceWatcher::watch(&path).ok();
                }

                self.refresh_markdown();
            }
            Err(error) => {
                self.status = format!("Failed to open {}: {error}", path.display());
            }
        }
    }

    fn open_folder(&mut self, path: PathBuf) {
        match build_tree(&path) {
            Ok(tree) => {
                self.tree = Some(tree);
                self.workspace_root = Some(path.clone());
                self.watcher = WorkspaceWatcher::watch(&path).ok();
                self.status = format!("Workspace loaded: {}", path.display());
            }
            Err(error) => {
                self.status = format!("Failed to open folder {}: {error}", path.display());
            }
        }
    }

    fn save_active_file(&mut self) {
        let Some(path) = self.editor.path().map(Path::to_path_buf) else {
            self.status = "No active file to save.".to_string();
            return;
        };

        match save_text_file(&path, &self.editor.text()) {
            Ok(()) => {
                self.editor.mark_saved();
                self.status = format!("Saved {}", path.display());
            }
            Err(error) => {
                self.status = format!("Failed to save {}: {error}", path.display());
            }
        }
    }

    fn handle_tree_press(&mut self, path: PathBuf) {
        if path.is_dir() {
            if let Some(tree) = &mut self.tree {
                if let Err(error) = expand_directory(tree, &path) {
                    self.status = format!("Failed to expand {}: {error}", path.display());
                }
            }

            return;
        }

        let now = Instant::now();
        let should_open = self.last_tree_click.as_ref().is_some_and(|(last_path, last_at)| {
            *last_path == path && now.duration_since(*last_at) <= Duration::from_millis(350)
        });

        self.last_tree_click = Some((path.clone(), now));

        if should_open {
            if supported_file(&path) {
                self.open_file(path);
            } else {
                let name = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                self.modal = Some(ModalState::UnsupportedFile {
                    message: format!("\"{}\" is not a supported file type.", name),
                    path,
                });
            }
        } else {
            self.status = "Double-click a file in the sidebar to open it.".to_string();
        }
    }

    fn poll_watcher(&mut self) {
        let Some(watcher) = &self.watcher else {
            return;
        };

        let events = watcher.drain();

        if events.is_empty() {
            return;
        }

        if let Some(tree) = &mut self.tree {
            let _ = refresh_tree(tree);
        }

        if let Some(active_file) = self.editor.path().map(Path::to_path_buf) {
            let touched_active_file = events
                .iter()
                .flatten()
                .any(|event| event.paths.iter().any(|event_path| event_path == &active_file));

            if touched_active_file && !self.editor.is_dirty() {
                if let Ok(contents) = read_text_file(&active_file) {
                    if contents != self.editor.text() {
                        self.editor.reload_from_disk(Some(active_file.clone()), contents);
                        self.refresh_markdown();
                        self.status = format!(
                            "Reloaded {} after external change.",
                            active_file.display()
                        );
                    }
                }
            }
        }
    }

    fn refresh_markdown(&mut self) {
        if is_markdown_file(self.editor.path()) {
            self.markdown_items = parse_items(&self.editor.text());
        } else {
            self.markdown_items.clear();
        }
    }

    fn sidebar_view(&self, tree: &FileNode) -> Element<'_, Message> {
        let palette = self.palette();
        let visible = visible_nodes(tree);
        let workspace_label = self
            .workspace_root
            .as_deref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| tree.name.clone());

        let header = container(
            column![
                text("WORKSPACE")
                    .font(Self::mono_font())
                    .size(9)
                    .color(palette.text_soft),
                row![
                    column![
                        text(workspace_label)
                            .font(Self::display_font())
                            .size(18)
                            .color(palette.text_strong),
                        text(format!("{} items", visible.len().saturating_sub(1)))
                            .font(Self::body_font())
                            .size(11)
                            .color(palette.text_soft),
                    ]
                    .spacing(2)
                    .width(Fill),
                    icon_button(palette, "\u{1F4C2}", Message::OpenFolder, true, "Open folder"),
                ]
                .align_y(Vertical::Center)
                .spacing(8),
            ]
            .spacing(8),
        )
        .height(header_bar_height())
        .padding([12, 14])
        .style(surface_header_style(palette, SectionTone::Sidebar));

        let mut nodes = column![].spacing(4);

        for node in visible.iter() {
            let is_active = self.editor.path().is_some_and(|path| path == node.path.as_path());
            let prefix = if node.is_dir {
                if node.expanded {
                    "\u{25BE}"
                } else {
                    "\u{25B8}"
                }
            } else {
                ""
            };

            let button = button(
                row![
                    text(prefix)
                        .font(Self::body_font())
                        .size(10)
                        .width(12)
                        .color(if node.is_dir {
                            palette.text_soft
                        } else {
                            Color::TRANSPARENT
                        }),
                    text(node.name.clone())
                        .font(if node.is_dir { Self::body_font() } else { Self::mono_font() })
                        .size(13)
                        .color(if is_active {
                            palette.accent_text
                        } else if node.is_dir {
                            palette.text_muted
                        } else {
                            palette.text_strong
                        })
                        .width(Fill),
                ]
                .align_y(Vertical::Center)
                .spacing(6),
            )
            .width(Fill)
            .padding([6, 10])
            .style(tree_button_style(palette, is_active))
            .on_press(Message::TreePressed(node.path.clone()));

            nodes = nodes.push(row![Space::new().width(tree_indent(node.depth)), button].width(Fill));
        }

        let sidebar_scroll = scrollable(nodes.spacing(6))
            .direction(minimal_scrollbar())
            .style(minimal_scrollable_style(palette, self.scrollbars_are_visible()))
            .on_scroll(|_| Message::ScrollActivity(Instant::now()))
            .width(Fill)
            .height(Fill);

        column![
            header,
            container(sidebar_scroll)
                .width(Fill)
                .height(Fill)
                .padding([12, 10])
                .style(section_body_style(palette, SectionTone::Sidebar, false))
        ]
        .spacing(content_section_gap())
        .width(SIDEBAR_WIDTH)
        .height(Fill)
        .into()
    }

    fn editor_view(&self) -> Element<'_, Message> {
        let palette = self.palette();
        let syntax_profile = syntax::detect(self.editor.path());
        let breadcrumb = breadcrumb_path(self.workspace_root.as_deref(), self.editor.path());
        let mut gutter = column![];

        for line in self.editor.line_numbers() {
            gutter = gutter.push(
                text(line)
                    .font(Self::mono_font())
                    .size(13)
                    .width(Length::Fixed(42.0))
                    .align_x(Horizontal::Right)
                    .color(palette.text_soft),
            );
        }

        let editor_header = container(
            row![
                column![
                    text("DOCUMENT")
                        .font(Self::mono_font())
                        .size(9)
                        .color(palette.text_soft),
                    text(self.editor.file_label())
                        .font(Self::display_font())
                        .size(18)
                        .color(palette.text_strong),
                    text(breadcrumb)
                        .font(Self::body_font())
                        .size(12)
                        .color(palette.text_soft),
                ]
                .spacing(2)
                .width(Fill),
                row![
                    info_pill(palette, syntax_profile.extension.to_uppercase(), false),
                    info_pill(
                        palette,
                        if self.editor.is_dirty() { "\u{25CF}" } else { "\u{2713}" },
                        self.editor.is_dirty(),
                    ),
                ]
                .spacing(6),
                if is_markdown_file(self.editor.path()) {
                    icon_button(
                        palette,
                        if self.preview_open {
                            "\u{25A8}"
                        } else {
                            "\u{25A3}"
                        },
                        Message::TogglePreview,
                        self.preview_open,
                        if self.preview_open {
                            "Hide preview"
                        } else {
                            "Show preview"
                        },
                    )
                } else {
                    icon_button(palette, "\u{1F4C4}", Message::OpenFile, false, "Open file")
                },
            ]
            .align_y(Vertical::Center)
            .spacing(10),
        )
        .height(header_bar_height())
        .padding([12, 14])
        .style(surface_header_style(palette, SectionTone::Editor));

        let editor = text_editor(self.editor.content())
            .font(Self::mono_font())
            .size(16)
            .line_height(1.45)
            .placeholder("Open a file or folder to start writing...")
            .key_binding(editor_key_binding)
            .on_action(Message::EditorAction)
            .style(text_editor_style(palette))
            .highlight(
                &syntax_profile.highlight_token,
                match self.theme_mode {
                    ThemeMode::Light => highlighter::Theme::InspiredGitHub,
                    ThemeMode::Dark => highlighter::Theme::SolarizedDark,
                },
            );

        let status = row![
            text(syntax_profile.syntax_name)
                .font(Self::mono_font())
                .size(11)
                .color(palette.text_soft),
            text(if self.editor.is_dirty() { "\u{25CF}" } else { "\u{25CB}" })
                .font(Self::body_font())
                .size(10)
                .color(if self.editor.is_dirty() {
                    palette.warning
                } else {
                    palette.text_soft
                }),
            text(self.status.clone())
                .font(Self::body_font())
                .size(11)
                .width(Fill)
                .color(palette.text_soft),
            text(format!(
                "{}:{}",
                self.editor.cursor_location().line,
                self.editor.cursor_location().column
            ))
            .font(Self::mono_font())
            .size(11)
            .color(palette.text_soft),
        ]
        .spacing(10)
        .align_y(Vertical::Center);

        let gutter_scroll = scrollable(gutter.spacing(8))
            .direction(minimal_scrollbar())
            .style(minimal_scrollable_style(palette, self.scrollbars_are_visible()))
            .on_scroll(|_| Message::ScrollActivity(Instant::now()))
            .height(Fill);

        let editor_body = container(
            column![
                row![
                    container(gutter_scroll).width(58).padding([12, 6]),
                    container(editor).width(Fill).height(Fill).padding([12, 16]),
                ]
                .height(Fill),
                separator(palette.divider),
                container(status).padding([8, 16])
            ]
            .height(Fill)
            .spacing(0),
        )
        .width(Fill)
        .height(Fill)
        .style(section_body_style(palette, SectionTone::Editor, true));

        column![editor_header, editor_body]
            .spacing(content_section_gap())
            .height(Fill)
            .width(Fill)
            .into()
    }

    fn preview_view(&self) -> Element<'_, Message> {
        let palette = self.palette();
        let preview = iced::widget::markdown::view(&self.markdown_items, self.theme())
            .map(Message::LinkClicked);

        let preview_header = container(
            row![
                column![
                    text("PREVIEW")
                        .font(Self::mono_font())
                        .size(9)
                        .color(palette.text_soft),
                    text("Markdown")
                        .font(Self::display_font())
                        .size(18)
                        .color(palette.text_strong),
                    text("Live render")
                        .font(Self::body_font())
                        .size(11)
                        .color(palette.text_soft),
                ]
                .spacing(2)
                .width(Fill),
                icon_button(
                    palette,
                    if self.preview_fullscreen {
                        "\u{25F0}"
                    } else {
                        "\u{25F1}"
                    },
                    Message::TogglePreviewFullscreen,
                    self.preview_fullscreen,
                    if self.preview_fullscreen {
                        "Show editor"
                    } else {
                        "Full preview"
                    },
                ),
            ]
            .align_y(Vertical::Center),
        )
        .height(header_bar_height())
        .padding([12, 14])
        .style(surface_header_style(palette, SectionTone::Preview));

        let preview_scroll = scrollable(container(preview).padding([8, 2]).width(Fill))
            .direction(minimal_scrollbar())
            .style(minimal_scrollable_style(palette, self.scrollbars_are_visible()))
            .on_scroll(|_| Message::ScrollActivity(Instant::now()))
            .width(Fill)
            .height(Fill);

        let preview_body = container(preview_scroll)
            .width(Fill)
            .height(Fill)
            .padding([12, 14])
            .style(section_body_style(palette, SectionTone::Preview, false));

        column![preview_header, preview_body]
            .spacing(content_section_gap())
            .width(if self.preview_fullscreen { Length::Fill } else { Length::Fixed(PREVIEW_WIDTH) })
            .height(Fill)
            .into()
    }

    fn display_font() -> Font {
        Font::with_name("Bahnschrift SemiBold")
    }

    fn body_font() -> Font {
        Font::with_name("Aptos")
    }

    fn mono_font() -> Font {
        Font::with_name("Cascadia Code")
    }

    fn palette(&self) -> Palette {
        match self.theme_mode {
            ThemeMode::Light => Palette::light(),
            ThemeMode::Dark => Palette::dark(),
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    background: Color,
    card: Color,
    card_strong: Color,
    border: Color,
    divider: Color,
    accent: Color,
    accent_text: Color,
    text_strong: Color,
    text_muted: Color,
    text_soft: Color,
    warning: Color,
    shadow: Color,
    danger: Color,
}

impl Palette {
    fn light() -> Self {
        Self {
            background: Color::from_rgb8(234, 241, 249),
            card: Color::from_rgba8(244, 249, 253, 0.86),
            card_strong: Color::from_rgba8(252, 254, 255, 0.95),
            border: Color::from_rgba8(108, 145, 176, 0.18),
            divider: Color::from_rgba8(112, 147, 178, 0.12),
            accent: Color::from_rgb8(78, 132, 188),
            accent_text: Color::from_rgb8(43, 89, 142),
            text_strong: Color::from_rgb8(39, 58, 78),
            text_muted: Color::from_rgb8(96, 118, 141),
            text_soft: Color::from_rgb8(139, 157, 176),
            warning: Color::from_rgb8(196, 145, 88),
            shadow: Color::from_rgba8(48, 73, 101, 0.10),
            danger: Color::from_rgb8(182, 92, 102),
        }
    }

    fn dark() -> Self {
        Self {
            background: Color::from_rgb8(20, 28, 40),
            card: Color::from_rgba8(30, 41, 56, 0.84),
            card_strong: Color::from_rgba8(37, 49, 67, 0.94),
            border: Color::from_rgba8(191, 214, 235, 0.13),
            divider: Color::from_rgba8(191, 214, 235, 0.10),
            accent: Color::from_rgb8(121, 176, 232),
            accent_text: Color::from_rgb8(220, 238, 255),
            text_strong: Color::from_rgb8(232, 240, 248),
            text_muted: Color::from_rgb8(165, 184, 204),
            text_soft: Color::from_rgb8(118, 138, 159),
            warning: Color::from_rgb8(227, 183, 121),
            shadow: Color::from_rgba8(0, 0, 0, 0.22),
            danger: Color::from_rgb8(232, 132, 132),
        }
    }
}

fn toggle_menu(current: Option<MenuKind>, clicked: MenuKind) -> Option<MenuKind> {
    match current {
        Some(open) if open == clicked => None,
        _ => Some(clicked),
    }
}

fn next_menu_hover_state(
    _current: MenuRegionState,
    _region: HoverRegion,
    hovered: bool,
) -> MenuRegionState {
    MenuRegionState {
        overlay_hovered: hovered,
    }
}

fn menu_should_close(state: MenuRegionState) -> bool {
    !state.overlay_hovered
}

fn menu_close_deadline(now: Instant, state: MenuRegionState) -> Option<Instant> {
    menu_should_close(state).then_some(now + menu_close_delay())
}

fn window_title_label(editor: &EditorBuffer) -> String {
    if editor.is_dirty() {
        format!("{} •", editor.file_label())
    } else {
        editor.file_label()
    }
}

fn breadcrumb_path(workspace_root: Option<&Path>, active_path: Option<&Path>) -> String {
    let Some(active_path) = active_path else {
        return "Untitled".to_string();
    };

    if let Some(root) = workspace_root {
        if let Ok(relative_path) = active_path.strip_prefix(root) {
            let segments = relative_path
                .iter()
                .map(|segment| segment.to_string_lossy().to_string())
                .collect::<Vec<_>>();

            if !segments.is_empty() {
                return segments.join(" / ");
            }
        }
    }

    active_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| active_path.display().to_string())
}

fn is_save_shortcut(event: &keyboard::Event) -> bool {
    matches!(
        event,
        keyboard::Event::KeyPressed {
            key: Key::Character(character),
            modifiers,
            ..
        } if command_pressed(*modifiers) && character.eq_ignore_ascii_case("s")
    )
}

fn editor_key_binding(
    key_press: text_editor::KeyPress,
) -> Option<text_editor::Binding<Message>> {
    editor_custom_binding(&key_press)
        .or_else(|| text_editor::Binding::from_key_press(key_press))
}

fn editor_custom_binding(
    key_press: &text_editor::KeyPress,
) -> Option<text_editor::Binding<Message>> {
    if !matches!(key_press.status, text_editor::Status::Focused { .. }) {
        return None;
    }

    match key_press.key.as_ref() {
        Key::Named(keyboard::key::Named::Tab) if key_press.modifiers.shift() => {
            Some(text_editor::Binding::Custom(Message::EditorAction(
                text_editor::Action::Edit(text_editor::Edit::Unindent),
            )))
        }
        Key::Named(keyboard::key::Named::Tab) => Some(text_editor::Binding::Custom(
            Message::EditorAction(text_editor::Action::Edit(text_editor::Edit::Indent)),
        )),
        Key::Character(character)
            if command_pressed(key_press.modifiers)
                && key_press.modifiers.shift()
                && character.eq_ignore_ascii_case("z") =>
        {
            Some(text_editor::Binding::Custom(Message::Redo))
        }
        Key::Character(character)
            if command_pressed(key_press.modifiers) && character.eq_ignore_ascii_case("z") =>
        {
            Some(text_editor::Binding::Custom(Message::Undo))
        }
        _ => None,
    }
}

fn is_escape_shortcut(event: &keyboard::Event) -> bool {
    matches!(
        event,
        keyboard::Event::KeyPressed {
            key: Key::Named(keyboard::key::Named::Escape),
            ..
        }
    )
}

fn command_pressed(modifiers: Modifiers) -> bool {
    modifiers.control() || modifiers.logo()
}

fn root_style(palette: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: None,
        background: Some(Background::Color(palette.background)),
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

fn titlebar_shell_style(palette: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: None,
        background: Some(Background::Color(with_alpha(palette.card_strong, 0.88))),
        border: Border {
            color: with_alpha(palette.border, 0.6),
            width: subtle_border_width(),
            radius: menu_shell_radius().into(),
        },
        shadow: Shadow {
            color: palette.shadow,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 10.0,
        },
        snap: false,
    }
}

fn titlebar_style(_palette: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: None,
        background: Some(Background::Color(Color::TRANSPARENT)),
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

fn brand_mark_style(palette: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: None,
        background: Some(Background::Color(palette.accent)),
        border: Border {
            color: with_alpha(palette.accent, 0.6),
            width: subtle_border_width(),
            radius: 0.0.into(),
        },
        shadow: Shadow {
            color: palette.shadow,
            offset: Vector::new(0.0, 3.0),
            blur_radius: 10.0,
        },
        snap: false,
    }
}

fn surface_header_style(
    palette: Palette,
    tone: SectionTone,
) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: None,
        background: Some(Background::Color(header_surface_color(palette, tone))),
        border: Border {
            color: with_alpha(palette.border, 0.72),
            width: subtle_border_width(),
            radius: control_radius().into(),
        },
        shadow: Shadow {
            color: palette.shadow,
            offset: Vector::new(0.0, 3.0),
            blur_radius: 10.0,
        },
        snap: false,
    }
}

fn section_body_style(
    palette: Palette,
    tone: SectionTone,
    elevated: bool,
) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: None,
        background: Some(Background::Color(body_surface_color(palette, tone))),
        border: Border {
            color: if elevated {
                with_alpha(palette.border, 0.9)
            } else {
                palette.border
            },
            width: subtle_border_width(),
            radius: card_radius().into(),
        },
        shadow: Shadow {
            color: palette.shadow,
            offset: Vector::new(0.0, if elevated { 8.0 } else { 5.0 }),
            blur_radius: if elevated { 18.0 } else { 12.0 },
        },
        snap: false,
    }
}

fn menu_panel_style(palette: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: None,
        background: Some(Background::Color(with_alpha(palette.card_strong, 0.98))),
        border: Border {
            color: with_alpha(palette.border, 0.7),
            width: subtle_border_width(),
            radius: card_radius().into(),
        },
        shadow: Shadow {
            color: palette.shadow,
            offset: Vector::new(0.0, 8.0),
            blur_radius: 16.0,
        },
        snap: false,
    }
}

fn modal_card_style(palette: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: None,
        background: Some(Background::Color(with_alpha(palette.card_strong, 0.99))),
        border: Border {
            color: with_alpha(palette.border, 0.85),
            width: subtle_border_width(),
            radius: menu_shell_radius().into(),
        },
        shadow: Shadow {
            color: palette.shadow,
            offset: Vector::new(0.0, 10.0),
            blur_radius: 22.0,
        },
        snap: false,
    }
}

fn modal_inline_panel_style(palette: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: None,
        background: Some(Background::Color(with_alpha(palette.accent, 0.08))),
        border: Border {
            color: with_alpha(palette.border, 0.55),
            width: subtle_border_width(),
            radius: menu_shell_radius().into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn dismiss_icon() -> &'static str {
    "×"
}

fn open_anyway_icon() -> &'static str {
    "↗"
}

fn about_repo_icon() -> &'static str {
    "↗"
}

fn tooltip_style(palette: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: None,
        background: Some(Background::Color(with_alpha(palette.card_strong, 0.96))),
        border: Border {
            color: with_alpha(palette.border, 0.5),
            width: subtle_border_width(),
            radius: 0.0.into(),
        },
        shadow: Shadow {
            color: palette.shadow,
            offset: Vector::new(0.0, 2.0),
            blur_radius: 6.0,
        },
        snap: false,
    }
}

fn tree_button_style(
    palette: Palette,
    is_active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let background = if is_active {
            with_alpha(palette.accent, 0.16)
        } else if hovered {
            with_alpha(palette.accent, 0.08)
        } else {
            Color::TRANSPARENT
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color: if is_active {
                palette.accent_text
            } else {
                palette.text_strong
            },
            border: Border {
                color: if is_active {
                    with_alpha(palette.border, 0.7)
                } else if hovered {
                    with_alpha(palette.border, 0.35)
                } else {
                    Color::TRANSPARENT
                },
                width: subtle_border_width(),
                radius: control_radius().into(),
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

fn menu_button(
    palette: Palette,
    kind: MenuKind,
    is_open: bool,
) -> iced::widget::Button<'static, Message> {
    let label = match kind {
        MenuKind::File => "File",
        MenuKind::View => "View",
        MenuKind::Help => "Help",
    };

    button(
        text(label)
            .font(App::body_font())
            .size(13)
            .color(if is_open {
                palette.accent_text
            } else {
                palette.text_strong
            }),
    )
    .width(MENU_BUTTON_WIDTH)
    .padding([6, 8])
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

        button::Style {
            background: Some(Background::Color(if is_open {
                with_alpha(palette.accent, 0.16)
            } else if hovered {
                with_alpha(palette.accent, 0.08)
            } else {
                Color::TRANSPARENT
            })),
            text_color: if is_open {
                palette.accent_text
            } else {
                palette.text_strong
            },
            border: Border {
                color: if is_open {
                    with_alpha(palette.border, 0.65)
                } else if hovered {
                    with_alpha(palette.border, 0.3)
                } else {
                    Color::TRANSPARENT
                },
                width: subtle_border_width(),
                radius: menu_shell_radius().into(),
            },
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .on_press(Message::ToggleMenu(kind))
}

fn menu_item_button(
    palette: Palette,
    label: &'static str,
    message: Message,
) -> iced::widget::Button<'static, Message> {
    button(
        text(label)
            .font(App::body_font())
            .size(14)
            .color(palette.text_strong),
    )
    .width(Fill)
    .padding([8, 10])
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

        button::Style {
            background: Some(Background::Color(if hovered {
                with_alpha(palette.accent, 0.08)
            } else {
                Color::TRANSPARENT
            })),
            text_color: palette.text_strong,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: control_radius().into(),
            },
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .on_press(message)
}

fn theme_toggle_button(
    palette: Palette,
    theme_mode: ThemeMode,
) -> Element<'static, Message> {
    icon_button(
        palette,
        match theme_mode {
            ThemeMode::Light => "\u{263E}",
            ThemeMode::Dark => "\u{2600}",
        },
        Message::ToggleTheme,
        true,
        match theme_mode {
            ThemeMode::Light => "Dark theme",
            ThemeMode::Dark => "Light theme",
        },
    )
}

fn icon_button(
    palette: Palette,
    symbol: &'static str,
    message: Message,
    emphasized: bool,
    hint: &'static str,
) -> Element<'static, Message> {
    let btn = button(
        text(symbol)
            .font(App::body_font())
            .size(15)
            .align_x(Horizontal::Center)
            .width(Fill)
            .color(if emphasized {
                palette.accent_text
            } else {
                palette.text_muted
            }),
    )
    .width(32)
    .padding([6, 0])
    .style(move |_, status| compact_button_style(palette, emphasized, status))
    .on_press(message);

    tooltip(
        btn,
        text(hint).font(App::body_font()).size(11).color(palette.text_muted),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .style(tooltip_style(palette))
    .into()
}

fn compact_button_style(
    palette: Palette,
    emphasized: bool,
    status: button::Status,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

    button::Style {
        background: Some(Background::Color(if emphasized {
            if hovered {
                with_alpha(palette.accent, 0.18)
            } else {
                with_alpha(palette.accent, 0.12)
            }
        } else if hovered {
            with_alpha(palette.accent, 0.08)
        } else {
            with_alpha(palette.card, 0.3)
        })),
        text_color: if emphasized {
            palette.accent_text
        } else {
            palette.text_muted
        },
        border: Border {
            color: if emphasized {
                with_alpha(palette.border, 0.6)
            } else {
                with_alpha(palette.border, 0.32)
            },
            width: subtle_border_width(),
            radius: control_radius().into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn info_pill(
    palette: Palette,
    label: impl Into<String>,
    emphasized: bool,
) -> iced::widget::Container<'static, Message> {
    let label = label.into();

    container(
        text(label)
            .font(App::mono_font())
            .size(10)
            .color(if emphasized {
                palette.accent_text
            } else {
                palette.text_soft
            }),
    )
    .padding([4, 8])
    .style(move |_| container::Style {
        text_color: None,
        background: Some(Background::Color(if emphasized {
            with_alpha(palette.accent, 0.12)
        } else {
            with_alpha(palette.card_strong, 0.65)
        })),
        border: Border {
            color: if emphasized {
                with_alpha(palette.border, 0.7)
            } else {
                with_alpha(palette.border, 0.38)
            },
            width: subtle_border_width(),
            radius: control_radius().into(),
        },
        shadow: Shadow::default(),
        snap: false,
    })
}

fn menu_info_line(
    palette: Palette,
    label: &'static str,
) -> iced::widget::Container<'static, Message> {
    container(
        text(label)
            .font(App::body_font())
            .size(12)
            .color(palette.text_soft),
    )
    .padding([4, 10])
}

fn window_control_button(
    palette: Palette,
    kind: WindowControlKind,
    label: &'static str,
    message: Message,
    hint: &'static str,
) -> Element<'static, Message> {
    let btn = button(
        text(label)
            .font(App::body_font())
            .size(15)
            .align_x(Horizontal::Center)
            .width(Fill),
    )
    .width(34)
    .padding([7, 0])
    .style(move |_, status| window_control_style(palette, kind, status))
    .on_press(message);

    tooltip(
        btn,
        text(hint).font(App::body_font()).size(11).color(palette.text_muted),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .style(tooltip_style(palette))
    .into()
}

fn window_control_style(
    palette: Palette,
    kind: WindowControlKind,
    status: button::Status,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let (background, text_color) = match kind {
        WindowControlKind::Close if hovered => (with_alpha(palette.danger, 0.18), palette.danger),
        WindowControlKind::Close => (Color::TRANSPARENT, palette.text_muted),
        _ if hovered => (with_alpha(palette.accent, 0.08), palette.text_strong),
        _ => (Color::TRANSPARENT, palette.text_muted),
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: control_radius().into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn text_editor_style(
    palette: Palette,
) -> impl Fn(&Theme, iced::widget::text_editor::Status) -> iced::widget::text_editor::Style {
    move |_, _status| {
        iced::widget::text_editor::Style {
            background: Background::Color(with_alpha(palette.card_strong, 0.28)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 0.0.into(),
            },
            placeholder: palette.text_soft,
            value: palette.text_strong,
            selection: with_alpha(palette.accent, 0.18),
        }
    }
}

fn minimal_scrollbar() -> iced::widget::scrollable::Direction {
    iced::widget::scrollable::Direction::Vertical(
        iced::widget::scrollable::Scrollbar::new()
            .width(4)
            .scroller_width(4)
            .margin(3),
    )
}

fn minimal_scrollable_style(
    palette: Palette,
    recent_scroll_activity: bool,
) -> impl Fn(&Theme, iced::widget::scrollable::Status) -> iced::widget::scrollable::Style {
    move |_, status| {
        let alpha = scrollbar_alpha(recent_scroll_activity, status);
        let rail = iced::widget::scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: iced::widget::scrollable::Scroller {
                background: Background::Color(with_alpha(palette.text_soft, alpha)),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 3.0.into(),
                },
            },
        };

        iced::widget::scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: iced::widget::scrollable::AutoScroll {
                background: Background::Color(with_alpha(palette.card_strong, 0.6)),
                border: Border {
                    color: with_alpha(palette.border, 0.35),
                    width: subtle_border_width(),
                    radius: control_radius().into(),
                },
                shadow: Shadow::default(),
                icon: palette.text_muted,
            },
        }
    }
}

fn menu_panel_offset(kind: MenuKind) -> f32 {
    match kind {
        MenuKind::File => 0.0,
        MenuKind::View => MENU_BUTTON_WIDTH + MENU_GAP,
        MenuKind::Help => (MENU_BUTTON_WIDTH + MENU_GAP) * 2.0,
    }
}

fn with_alpha(color: Color, alpha: f32) -> Color {
    Color::from_rgba(color.r, color.g, color.b, alpha)
}

fn blend_colors(base: Color, tint: Color, amount: f32) -> Color {
    let keep = 1.0 - amount;

    Color::from_rgba(
        base.r * keep + tint.r * amount,
        base.g * keep + tint.g * amount,
        base.b * keep + tint.b * amount,
        base.a * keep + tint.a * amount,
    )
}

fn header_surface_color(palette: Palette, tone: SectionTone) -> Color {
    match tone {
        SectionTone::Sidebar => blend_colors(palette.card_strong, palette.accent, 0.070),
        SectionTone::Editor => blend_colors(palette.card_strong, palette.accent, 0.028),
        SectionTone::Preview => blend_colors(palette.card_strong, palette.accent, 0.050),
    }
}

fn body_surface_color(palette: Palette, tone: SectionTone) -> Color {
    match tone {
        SectionTone::Sidebar => blend_colors(palette.card, palette.accent, 0.050),
        SectionTone::Editor => blend_colors(palette.card_strong, palette.accent, 0.018),
        SectionTone::Preview => blend_colors(palette.card, palette.accent, 0.034),
    }
}

fn content_section_gap() -> f32 {
    0.0
}

fn menu_close_delay() -> Duration {
    Duration::from_millis(140)
}

fn tree_indent(depth: usize) -> f32 {
    let depth = depth.min(7) as f32;
    depth * 12.0
}

fn menu_overlay_offset_y() -> f32 {
    TITLEBAR_INSET_Y * 2.0 + 38.0
}

fn menu_shell_radius() -> f32 {
    10.0
}

fn header_bar_height() -> f32 {
    72.0
}

fn card_radius() -> f32 {
    0.0
}

fn control_radius() -> f32 {
    0.0
}

fn subtle_border_width() -> f32 {
    0.6
}

fn scrollbar_alpha(
    recent_scroll_activity: bool,
    status: iced::widget::scrollable::Status,
) -> f32 {
    if !scrollbar_is_visible(recent_scroll_activity, status) {
        return 0.0;
    }

    match status {
        iced::widget::scrollable::Status::Active { .. } => 0.24,
        iced::widget::scrollable::Status::Hovered { .. } => 0.34,
        iced::widget::scrollable::Status::Dragged { .. } => 0.5,
    }
}

fn scrollbar_is_visible(
    recent_scroll_activity: bool,
    status: iced::widget::scrollable::Status,
) -> bool {
    match status {
        iced::widget::scrollable::Status::Active { .. } => recent_scroll_activity,
        iced::widget::scrollable::Status::Hovered {
            is_horizontal_scrollbar_hovered,
            is_vertical_scrollbar_hovered,
            ..
        } => {
            recent_scroll_activity
                || is_horizontal_scrollbar_hovered
                || is_vertical_scrollbar_hovered
        }
        iced::widget::scrollable::Status::Dragged { .. } => true,
    }
}

fn about_body_message() -> &'static str {
    "MyPad is a lightweight Rust editor made by Samuel Jarai. GitHub: https://github.com/jaggerjack61/MyPad"
}

fn about_repo_url() -> &'static str {
    "https://github.com/jaggerjack61/MyPad"
}

fn separator(color: Color) -> iced::widget::Container<'static, Message> {
    container(Space::new().height(1))
        .width(Fill)
        .height(1)
        .style(move |_| container::Style {
            text_color: None,
            background: Some(Background::Color(color)),
            border: Border::default(),
            shadow: Shadow::default(),
            snap: false,
        })
}

#[cfg(test)]
mod tests {
    use super::{
        about_body_message, about_repo_icon, body_surface_color, breadcrumb_path, card_radius,
        content_section_gap, control_radius, header_bar_height, menu_close_deadline,
        dismiss_icon, menu_close_delay, menu_overlay_offset_y, menu_shell_radius,
        menu_should_close, next_menu_hover_state, open_anyway_icon, scrollbar_alpha,
        subtle_border_width, toggle_menu, tree_indent, window_title_label,
        HoverRegion, MenuKind, MenuRegionState, ModalState, Palette, SectionTone,
        TITLEBAR_INSET_Y,
    };
    use crate::editor::EditorBuffer;
    use iced::keyboard::{self, Key, Modifiers};
    use iced::widget::text_editor;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    #[test]
    fn visual_tokens_stay_restrained() {
        assert_eq!(card_radius(), 0.0);
        assert_eq!(control_radius(), 0.0);
        assert!(menu_shell_radius() > 0.0);
        assert!(subtle_border_width() < 1.0);
    }

    #[test]
    fn about_copy_mentions_author_and_repo() {
        assert_eq!(
            about_body_message(),
            "MyPad is a lightweight Rust editor made by Samuel Jarai. GitHub: https://github.com/jaggerjack61/MyPad"
        );
    }

    #[test]
    fn modal_actions_use_icon_glyphs() {
        assert_eq!(dismiss_icon(), "×");
        assert_eq!(open_anyway_icon(), "↗");
        assert_eq!(about_repo_icon(), "↗");
    }

    #[test]
    fn show_about_opens_about_modal() {
        let (mut app, _) = super::App::new(None);

        let _ = app.update(super::Message::ShowAbout);

        assert_eq!(app.modal, Some(ModalState::About));
    }

    #[test]
    fn dismiss_modal_closes_about_modal() {
        let (mut app, _) = super::App::new(None);
        let _ = app.update(super::Message::ShowAbout);

        let _ = app.update(super::Message::DismissModal);

        assert_eq!(app.modal, None);
    }

    #[test]
    fn scrollbar_is_hidden_without_recent_activity() {
        let alpha = scrollbar_alpha(
            false,
            iced::widget::scrollable::Status::Active {
                is_horizontal_scrollbar_disabled: false,
                is_vertical_scrollbar_disabled: false,
            },
        );

        assert_eq!(alpha, 0.0);
    }

    #[test]
    fn scrollbar_appears_after_scrolling() {
        let alpha = scrollbar_alpha(
            true,
            iced::widget::scrollable::Status::Active {
                is_horizontal_scrollbar_disabled: false,
                is_vertical_scrollbar_disabled: false,
            },
        );

        assert!(alpha > 0.0);
    }

    #[test]
    fn toggling_same_menu_closes_it() {
        assert_eq!(toggle_menu(Some(MenuKind::File), MenuKind::File), None);
    }

    #[test]
    fn toggling_other_menu_switches_focus() {
        assert_eq!(toggle_menu(Some(MenuKind::File), MenuKind::View), Some(MenuKind::View));
    }

    #[test]
    fn titlebar_label_reflects_dirty_state() {
        let buffer = EditorBuffer::new(Some(PathBuf::from("notes.md")), "hello");

        assert_eq!(window_title_label(&buffer), "notes.md");
    }

    #[test]
    fn breadcrumb_path_prefers_workspace_relative_paths() {
        let breadcrumb = breadcrumb_path(
            Some(Path::new("C:/Users/demo/Desktop")),
            Some(Path::new("C:/Users/demo/Desktop/docs/reason.md")),
        );

        assert_eq!(breadcrumb, "docs / reason.md");
    }

    #[test]
    fn breadcrumb_path_falls_back_to_file_name() {
        let breadcrumb = breadcrumb_path(None, Some(Path::new("C:/temp/scratch.md")));

        assert_eq!(breadcrumb, "scratch.md");
    }

    #[test]
    fn header_bar_height_stays_shared() {
        assert_eq!(header_bar_height(), 72.0);
    }

    #[test]
    fn light_palette_uses_cool_blue_accent() {
        let palette = Palette::light();

        assert!(palette.accent.b > palette.accent.r);
        assert!(palette.accent_text.b > palette.accent_text.r);
    }

    #[test]
    fn content_sections_have_no_gap() {
        assert_eq!(content_section_gap(), 0.0);
    }

    #[test]
    fn section_surfaces_use_distinct_tones() {
        let palette = Palette::light();
        let sidebar = body_surface_color(palette, SectionTone::Sidebar);
        let editor = body_surface_color(palette, SectionTone::Editor);
        let preview = body_surface_color(palette, SectionTone::Preview);

        assert_ne!(sidebar, editor);
        assert_ne!(editor, preview);
    }

    #[test]
    fn menu_overlay_starts_below_menubar() {
        assert!(menu_overlay_offset_y() > 0.0);
        assert!(menu_overlay_offset_y() > TITLEBAR_INSET_Y);
    }

    #[test]
    fn tree_indent_caps_for_deep_paths() {
        assert!(tree_indent(2) < tree_indent(8));
        assert_eq!(tree_indent(8), tree_indent(40));
        assert!(tree_indent(40) <= 84.0);
    }

    #[test]
    fn menu_stays_open_while_overlay_is_hovered() {
        let state = MenuRegionState {
            overlay_hovered: true,
        };

        assert!(!menu_should_close(state));
    }

    #[test]
    fn menu_closes_when_overlay_is_left() {
        let state = MenuRegionState {
            overlay_hovered: true,
        };

        let state = next_menu_hover_state(state, HoverRegion::Overlay, false);

        assert!(!state.overlay_hovered);
        assert!(menu_should_close(state));
    }

    #[test]
    fn menu_exit_schedules_delayed_close() {
        let now = Instant::now();
        let state = MenuRegionState {
            overlay_hovered: false,
        };

        let deadline = menu_close_deadline(now, state).expect("deadline");

        assert_eq!(deadline, now + menu_close_delay());
    }

    #[test]
    fn overlay_hovered_menu_does_not_schedule_close() {
        let now = Instant::now();
        let state = MenuRegionState {
            overlay_hovered: true,
        };

        assert_eq!(menu_close_deadline(now, state), None);
    }

    // ── Cause 1: overlay exit closes menu ────────────────────────────

    #[test]
    fn cause1_overlay_exit_closes_menu() {
        let state = MenuRegionState {
            overlay_hovered: false,
        };

        assert!(
            menu_should_close(state),
            "menu should close when overlay is exited"
        );
    }

    // ── Cause 2: initial state allows eventual close ─────────────────

    #[test]
    fn cause2_initial_state_allows_eventual_close() {
        // The initial state from ToggleMenu: overlay_hovered:true
        let initial = MenuRegionState {
            overlay_hovered: true,
        };

        // Menu is open while overlay is hovered
        assert!(!menu_should_close(initial));

        // Overlay exit fires
        let state = next_menu_hover_state(initial, HoverRegion::Overlay, false);
        assert!(
            menu_should_close(state),
            "menu should close after overlay exit"
        );
    }

    // ── Cause 3: initial overlay_hovered:true prevents premature close ─

    #[test]
    fn cause3_initial_overlay_true_gives_grace() {
        // ToggleMenu now sets overlay_hovered:true, so even if on_enter
        // hasn't fired yet, the menu won't start closing immediately.
        let initial = MenuRegionState {
            overlay_hovered: true,
        };

        assert!(
            !menu_should_close(initial),
            "menu should NOT close immediately after opening"
        );

        let now = Instant::now();
        assert!(
            menu_close_deadline(now, initial).is_none(),
            "no close deadline while overlay is hovered"
        );
    }

    // ── Cause 4: close works with only overlay tracking ──────────────

    #[test]
    fn cause4_close_works_with_overlay_tracking() {
        let state = MenuRegionState {
            overlay_hovered: false,
        };

        assert!(
            menu_should_close(state),
            "close should work based on overlay_hovered alone"
        );
    }

    // ── Cause 5: exhaustive should_close truth table ─────────────────

    #[test]
    fn cause5_exhaustive_should_close_truth_table() {
        let cases = [
            (false, true),   // overlay left → close
            (true,  false),  // on overlay → stay
        ];

        for (overlay, expected) in cases {
            let state = MenuRegionState {
                overlay_hovered: overlay,
            };
            assert_eq!(
                menu_should_close(state),
                expected,
                "should_close(overlay={overlay}) expected {expected}"
            );
        }
    }

    // ── Cause 6: grace period timing ─────────────────────────────────

    #[test]
    fn cause6_grace_period_is_adequate() {
        let delay = menu_close_delay();
        assert!(
            delay.as_millis() >= 120,
            "Grace period {}ms may be too short for cursor transit",
            delay.as_millis()
        );
        assert!(
            delay.as_millis() <= 500,
            "Grace period {}ms is too long, feels sluggish",
            delay.as_millis()
        );
    }

    // ── Cause 7: deadline NOT set when region re-entered ─────────────

    #[test]
    fn cause7_re_entering_overlay_cancels_deadline() {
        let now = Instant::now();

        // Unhovered → deadline set
        let state_out = MenuRegionState {
            overlay_hovered: false,
        };
        let deadline = menu_close_deadline(now, state_out);
        assert!(deadline.is_some(), "deadline should be set when overlay is out");

        // Mouse re-enters overlay → deadline should cancel
        let state_in = next_menu_hover_state(state_out, HoverRegion::Overlay, true);
        let deadline = menu_close_deadline(now, state_in);
        assert!(
            deadline.is_none(),
            "Cause 7: deadline must be cancelled when overlay is re-entered"
        );
    }

    // ── Preview fullscreen toggle ────────────────────────────────────

    #[test]
    fn preview_fullscreen_defaults_to_off() {
        let (app, _) = super::App::new(None);
        assert!(!app.preview_fullscreen);
        assert!(app.preview_open);
    }

    #[test]
    fn toggling_preview_fullscreen_enables_preview() {
        let (mut app, _) = super::App::new(None);
        app.preview_open = false;
        let _ = app.update(super::Message::TogglePreviewFullscreen);
        assert!(app.preview_fullscreen);
        assert!(app.preview_open, "fullscreen should force preview_open");
    }

    #[test]
    fn hiding_preview_clears_fullscreen() {
        let (mut app, _) = super::App::new(None);
        app.preview_fullscreen = true;
        app.preview_open = true;
        let _ = app.update(super::Message::TogglePreview);
        assert!(!app.preview_open);
        assert!(!app.preview_fullscreen, "hiding preview should exit fullscreen");
    }

    #[test]
    fn editor_key_binding_maps_tab_to_indent() {
        let binding = super::editor_key_binding(key_press(
            Key::Named(keyboard::key::Named::Tab),
            Modifiers::default(),
        ));

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(super::Message::EditorAction(
                text_editor::Action::Edit(text_editor::Edit::Indent)
            )))
        ));
    }

    #[test]
    fn editor_key_binding_maps_shift_tab_to_unindent() {
        let binding = super::editor_key_binding(key_press(
            Key::Named(keyboard::key::Named::Tab),
            Modifiers::SHIFT,
        ));

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(super::Message::EditorAction(
                text_editor::Action::Edit(text_editor::Edit::Unindent)
            )))
        ));
    }

    #[test]
    fn editor_key_binding_maps_ctrl_z_to_undo() {
        let binding = super::editor_key_binding(key_press(
            Key::Character("z".into()),
            Modifiers::CTRL,
        ));

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(super::Message::Undo))
        ));
    }

    #[test]
    fn editor_key_binding_maps_ctrl_shift_z_to_redo() {
        let binding = super::editor_key_binding(key_press(
            Key::Character("z".into()),
            Modifiers::CTRL | Modifiers::SHIFT,
        ));

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(super::Message::Redo))
        ));
    }

    fn key_press(key: Key, modifiers: Modifiers) -> text_editor::KeyPress {
        let modified_key = if modifiers.shift() {
            match &key {
                Key::Character(character) => Key::Character(character.to_uppercase().into()),
                _ => key.clone(),
            }
        } else {
            key.clone()
        };

        text_editor::KeyPress {
            key,
            modified_key,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyZ),
            modifiers,
            text: None,
            status: text_editor::Status::Focused { is_hovered: false },
        }
    }
}
