#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod theme;
mod transport_worker;

use amri_core::{
    evaluate_protection, ui_text, Language, ProtectionSignals, ProtectionState, UiMessage,
};
use amri_subscriptions::{parse_subscription_text, ImportedNode};
use eframe::egui::{self, Align, Color32, Layout, RichText, Stroke, Vec2};
use std::path::PathBuf;
use std::time::Duration;
use transport_worker::{TransportUiState, TransportWorker};

fn amri_window_icon() -> egui::IconData {
    let image = image::load_from_memory(include_bytes!("../../../assets/brand/amri-icon.png"))
        .expect("canonical AMRI app icon PNG must remain valid");
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();

    egui::IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    }
}

fn brand_button(
    ui: &mut egui::Ui,
    source: egui::ImageSource<'static>,
    size: f32,
    alt: &str,
) -> egui::Response {
    ui.add(
        egui::Image::new(source)
            .fit_to_exact_size(Vec2::splat(size))
            .alt_text(alt)
            .sense(egui::Sense::click()),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .on_hover_text(alt)
}

fn desktop_background(ui: &mut egui::Ui) {
    let viewport = ui.max_rect();
    let source_ratio = 1920.0 / 1080.0;
    let viewport_ratio = viewport.width() / viewport.height().max(1.0);
    let size = if viewport_ratio > source_ratio {
        Vec2::new(viewport.width(), viewport.width() / source_ratio)
    } else {
        Vec2::new(viewport.height() * source_ratio, viewport.height())
    };
    let rect = egui::Rect::from_center_size(viewport.center(), size);
    egui::Image::new(egui::include_image!(
        "../../../assets/brand/background-desktop.svg"
    ))
    .fit_to_exact_size(size)
    .paint_at(ui, rect);
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AMRI VPN")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([980.0, 680.0])
            .with_icon(amri_window_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "AMRI VPN",
        options,
        Box::new(|cc| Ok(Box::new(AmriApp::new(cc)))),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Routes,
    Subscriptions,
    Rules,
    Settings,
}

struct AmriApp {
    page: Page,
    navigation_history: Vec<Page>,
    language: Language,
    language_menu_open: bool,
    transport: TransportWorker,
    transport_state: TransportUiState,
    subscription_input: String,
    imported_nodes: Vec<ImportedNode>,
    selected_node: usize,
    editing_node: Option<usize>,
    delete_confirmation: Option<usize>,
    info_node: Option<usize>,
    node_actions_open: bool,
    core_path: String,
    local_port: String,
    smart_routing: bool,
    kill_switch: bool,
    learning: bool,
    federated_learning: bool,
    background_probing: bool,
}

impl AmriApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = theme::PANEL;
        visuals.window_fill = theme::WINDOW;
        visuals.extreme_bg_color = theme::BACKDROP;
        visuals.faint_bg_color = theme::SURFACE_MUTED;
        visuals.selection.bg_fill = theme::ACCENT;
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(theme::CONTROL_RADIUS);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(theme::CONTROL_RADIUS);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(theme::CONTROL_RADIUS);
        visuals.widgets.noninteractive.corner_radius =
            egui::CornerRadius::same(theme::CONTROL_RADIUS);
        cc.egui_ctx.set_visuals(visuals);

        let mut style = (*cc.egui_ctx.style_of(egui::Theme::Dark)).clone();
        style.spacing.item_spacing = theme::item_spacing();
        style.spacing.button_padding = theme::button_padding();
        cc.egui_ctx.set_style_of(egui::Theme::Dark, style);

        let language = std::env::var("LANG")
            .map(|tag| Language::from_tag(&tag))
            .unwrap_or(Language::English);

        Self {
            page: Page::Home,
            navigation_history: Vec::new(),
            language,
            language_menu_open: false,
            transport: TransportWorker::new(),
            transport_state: TransportUiState::Idle,
            subscription_input: String::new(),
            imported_nodes: Vec::new(),
            selected_node: 0,
            editing_node: None,
            delete_confirmation: None,
            info_node: None,
            node_actions_open: false,
            core_path: std::env::var("AMRI_SING_BOX_PATH")
                .unwrap_or_else(|_| "sing-box.exe".into()),
            local_port: "20800".into(),
            smart_routing: true,
            kill_switch: true,
            learning: true,
            federated_learning: false,
            background_probing: true,
        }
    }

    fn transport_ready(&self) -> bool {
        matches!(&self.transport_state, TransportUiState::Ready { .. })
    }

    fn navigate_to(&mut self, page: Page) {
        if self.page != page {
            self.navigation_history.push(self.page);
            if self.navigation_history.len() > 16 {
                self.navigation_history.remove(0);
            }
            self.page = page;
        }
        self.language_menu_open = false;
    }

    fn navigate_back(&mut self) {
        self.page = self.navigation_history.pop().unwrap_or(Page::Home);
        self.language_menu_open = false;
    }

    fn refresh_transport_state(&mut self, ctx: &egui::Context) {
        if let Some(state) = self.transport.latest_state() {
            self.transport_state = state;
        }
        if matches!(&self.transport_state, TransportUiState::Connecting) {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    fn import_subscription_text(&mut self) {
        let parsed = parse_subscription_text("windows-manual", &self.subscription_input);
        if parsed.is_empty() {
            self.transport_state =
                TransportUiState::Failed("no supported VPN nodes were found".into());
            return;
        }

        if let Some(index) = self.editing_node {
            if parsed.len() != 1 || index >= self.imported_nodes.len() {
                self.transport_state = TransportUiState::Failed(
                    "editing requires exactly one supported VPN node link".into(),
                );
                return;
            }
            self.imported_nodes[index] = parsed.into_iter().next().expect("one parsed node");
            self.selected_node = index;
            self.editing_node = None;
        } else {
            self.imported_nodes = parsed;
            self.selected_node = 0;
        }

        self.delete_confirmation = None;
        self.info_node = None;
        self.node_actions_open = false;
        self.subscription_input.clear();
        if matches!(&self.transport_state, TransportUiState::Failed(_)) {
            self.transport_state = TransportUiState::Idle;
        }
    }

    fn begin_edit_selected_node(&mut self) {
        let Some(node) = self.imported_nodes.get(self.selected_node) else {
            return;
        };
        self.subscription_input = node.raw_uri.clone();
        self.editing_node = Some(self.selected_node);
        self.delete_confirmation = None;
        self.info_node = None;
        self.node_actions_open = false;
    }

    fn cancel_node_edit(&mut self) {
        self.editing_node = None;
        self.subscription_input.clear();
        self.delete_confirmation = None;
        self.info_node = None;
        self.node_actions_open = false;
    }

    fn delete_confirmed_node(&mut self) {
        let Some(index) = self.delete_confirmation.take() else {
            return;
        };
        if index >= self.imported_nodes.len() {
            return;
        }

        self.imported_nodes.remove(index);
        self.info_node = None;
        self.node_actions_open = false;
        match self.editing_node {
            Some(editing) if editing == index => {
                self.editing_node = None;
                self.subscription_input.clear();
            }
            Some(editing) if editing > index => self.editing_node = Some(editing - 1),
            _ => {}
        }

        if self.imported_nodes.is_empty() {
            self.selected_node = 0;
        } else {
            self.selected_node = index.min(self.imported_nodes.len() - 1);
        }
    }

    fn start_transport(&mut self) {
        let Some(node) = self.imported_nodes.get(self.selected_node).cloned() else {
            self.navigate_to(Page::Subscriptions);
            return;
        };
        let port = match self.local_port.trim().parse::<u16>() {
            Ok(port) if port != 0 => port,
            _ => {
                self.transport_state =
                    TransportUiState::Failed("local port must be between 1 and 65535".into());
                return;
            }
        };

        match self
            .transport
            .connect(node, PathBuf::from(self.core_path.trim()), port)
        {
            Ok(()) => self.transport_state = TransportUiState::Connecting,
            Err(error) => self.transport_state = TransportUiState::Failed(error),
        }
    }

    fn stop_transport(&mut self) {
        match self.transport.disconnect() {
            Ok(()) => self.transport_state = TransportUiState::Connecting,
            Err(error) => self.transport_state = TransportUiState::Failed(error),
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.set_width(220.0);
        ui.add_space(8.0);
        ui.label(RichText::new("AMRI").size(28.0).strong());
        ui.label(
            RichText::new(ui_text(self.language, UiMessage::AdaptiveVpn))
                .color(Color32::from_gray(145)),
        );
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            if brand_button(
                ui,
                egui::include_image!("../../../assets/brand/language-button.svg"),
                38.0,
                ui_text(self.language, UiMessage::LanguageLabel),
            )
            .clicked()
            {
                self.language_menu_open = !self.language_menu_open;
            }
            ui.label(RichText::new(self.language.native_name()).size(13.0));

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if brand_button(
                    ui,
                    egui::include_image!("../../../assets/brand/settings-button.svg"),
                    38.0,
                    ui_text(self.language, UiMessage::Settings),
                )
                .clicked()
                {
                    self.navigate_to(Page::Settings);
                }
            });
        });

        if self.language_menu_open {
            egui::Frame::new()
                .fill(Color32::from_rgb(22, 29, 42))
                .corner_radius(14)
                .inner_margin(10)
                .show(ui, |ui| {
                    for language in Language::ALL {
                        let selected = self.language == language;
                        if ui
                            .add(
                                egui::Button::selectable(selected, language.native_name())
                                    .min_size(Vec2::new(178.0, 30.0)),
                            )
                            .clicked()
                        {
                            self.language = language;
                            self.language_menu_open = false;
                        }
                    }
                });
        }

        ui.add_space(20.0);
        let language = self.language;
        self.nav_button(ui, Page::Home, ui_text(language, UiMessage::Home));
        self.nav_button(ui, Page::Routes, ui_text(language, UiMessage::Routes));
        self.nav_button(
            ui,
            Page::Subscriptions,
            ui_text(language, UiMessage::Subscriptions),
        );
        self.nav_button(ui, Page::Rules, ui_text(language, UiMessage::Rules));

        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.add_space(8.0);
            ui.label(
                RichText::new("Local Intelligence Engine")
                    .size(12.0)
                    .color(Color32::from_gray(125)),
            );
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(rect.center(), 4.0, Color32::from_rgb(70, 210, 130));
                ui.label(RichText::new(ui_text(self.language, UiMessage::EngineReady)).size(12.0));
            });
        });
    }

    fn nav_button(&mut self, ui: &mut egui::Ui, page: Page, text: &str) {
        let selected = self.page == page;
        let fill = if selected {
            Color32::from_rgb(41, 52, 82)
        } else {
            Color32::TRANSPARENT
        };
        let response = ui.add_sized(
            [196.0, 42.0],
            egui::Button::new(RichText::new(text).size(15.0))
                .fill(fill)
                .stroke(Stroke::NONE)
                .corner_radius(14),
        );
        if response.clicked() {
            self.navigate_to(page);
        }
    }

    fn toggle_row(ui: &mut egui::Ui, label: &str, description: &str, value: &mut bool) {
        egui::Frame::new()
            .fill(Color32::from_rgb(22, 27, 36))
            .corner_radius(16)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(label).size(15.0).strong());
                        ui.label(
                            RichText::new(description)
                                .size(12.0)
                                .color(Color32::from_gray(145)),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let state_text = if *value { "ON" } else { "OFF" };
                        ui.toggle_value(value, state_text);
                    });
                });
            });
    }

    fn metric_card(ui: &mut egui::Ui, title: &str, value: &str, subtitle: &str) {
        egui::Frame::new()
            .fill(theme::SURFACE)
            .corner_radius(theme::CARD_RADIUS)
            .inner_margin(theme::CARD_MARGIN)
            .show(ui, |ui| {
                ui.set_min_width(180.0);
                ui.label(
                    RichText::new(title)
                        .size(12.0)
                        .color(Color32::from_gray(145)),
                );
                ui.add_space(6.0);
                ui.label(RichText::new(value).size(26.0).strong());
                ui.add_space(2.0);
                ui.label(
                    RichText::new(subtitle)
                        .size(12.0)
                        .color(Color32::from_gray(125)),
                );
            });
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new(ui_text(self.language, UiMessage::Home)).size(30.0));
        ui.label(
            RichText::new(ui_text(self.language, UiMessage::SmartRoutingDescription))
                .color(Color32::from_gray(150)),
        );
        ui.add_space(18.0);

        egui::Frame::new()
            .fill(theme::HERO_SURFACE)
            .corner_radius(theme::HERO_RADIUS)
            .inner_margin(theme::HERO_MARGIN)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        let transport_ready =
                            matches!(&self.transport_state, TransportUiState::Ready { .. });
                        let protection = evaluate_protection(ProtectionSignals {
                            requested: !matches!(&self.transport_state, TransportUiState::Idle),
                            transport_ready,
                            // These remain false until the Windows TUN/DNS/leak adapters report
                            // readiness for the same route generation.
                            packet_forwarding_active: false,
                            dns_protection_ready: false,
                            leak_protection_ready: false,
                            public_egress_verified: false,
                        });
                        let protection_message = if protection.state == ProtectionState::Protected {
                            UiMessage::ProtectionOn
                        } else {
                            UiMessage::ProtectionOff
                        };
                        ui.label(
                            RichText::new(ui_text(self.language, protection_message))
                                .size(24.0)
                                .strong(),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(match &self.transport_state {
                                TransportUiState::Idle => {
                                    ui_text(self.language, UiMessage::AddSubscriptionFirst).into()
                                }
                                TransportUiState::Connecting => {
                                    ui_text(self.language, UiMessage::TransportConnecting).into()
                                }
                                TransportUiState::Ready {
                                    node_name,
                                    local_port,
                                    ..
                                } => format!(
                                    "{} · {} · 127.0.0.1:{}",
                                    ui_text(self.language, UiMessage::TransportReady),
                                    node_name,
                                    local_port
                                ),
                                TransportUiState::Failed(error) => error.clone(),
                            })
                            .color(Color32::from_gray(155)),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if self.transport_ready() {
                            ui_text(self.language, UiMessage::Disconnect)
                        } else {
                            ui_text(self.language, UiMessage::Connect)
                        };
                        let enabled =
                            !matches!(&self.transport_state, TransportUiState::Connecting);
                        let source = if self.transport_ready() {
                            egui::include_image!("../../../assets/brand/vpn-power-on.svg")
                        } else {
                            egui::include_image!("../../../assets/brand/vpn-power-off.svg")
                        };
                        let button = egui::Image::new(source)
                            .fit_to_exact_size(Vec2::splat(104.0))
                            .alt_text(label)
                            .sense(egui::Sense::click());
                        let response = ui
                            .add_enabled(enabled, button)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(label);
                        if response.clicked() {
                            if self.transport_ready() {
                                self.stop_transport();
                            } else if self.imported_nodes.is_empty() {
                                self.navigate_to(Page::Subscriptions);
                            } else {
                                self.start_transport();
                            }
                        }
                    });
                });
            });

        ui.add_space(18.0);
        ui.horizontal_wrapped(|ui| {
            Self::metric_card(
                ui,
                ui_text(self.language, UiMessage::ActiveRoutes),
                if self.transport_ready() { "1" } else { "0" },
                "",
            );
            Self::metric_card(ui, ui_text(self.language, UiMessage::AveragePing), "—", "");
            Self::metric_card(ui, ui_text(self.language, UiMessage::Jitter), "—", "");
            Self::metric_card(ui, ui_text(self.language, UiMessage::RouteScore), "—", "");
        });

        ui.add_space(22.0);
        ui.label(
            RichText::new(ui_text(self.language, UiMessage::SmartRouting))
                .size(20.0)
                .strong(),
        );
        ui.add_space(8.0);

        Self::toggle_row(
            ui,
            ui_text(self.language, UiMessage::SmartRouting),
            ui_text(self.language, UiMessage::SmartRoutingDescription),
            &mut self.smart_routing,
        );
        Self::toggle_row(
            ui,
            ui_text(self.language, UiMessage::LocalLearning),
            ui_text(self.language, UiMessage::LocalLearningDescription),
            &mut self.learning,
        );
        Self::toggle_row(
            ui,
            ui_text(self.language, UiMessage::FederatedLearning),
            ui_text(self.language, UiMessage::FederatedDescription),
            &mut self.federated_learning,
        );
        Self::toggle_row(
            ui,
            ui_text(self.language, UiMessage::BackgroundTesting),
            ui_text(self.language, UiMessage::BackgroundDescription),
            &mut self.background_probing,
        );
        Self::toggle_row(
            ui,
            ui_text(self.language, UiMessage::KillSwitch),
            ui_text(self.language, UiMessage::KillSwitchDescription),
            &mut self.kill_switch,
        );
    }

    fn routes(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new(ui_text(self.language, UiMessage::Routes)).size(30.0));
        ui.add_space(12.0);

        egui::Frame::new()
            .fill(Color32::from_rgb(22, 27, 36))
            .corner_radius(18)
            .inner_margin(18)
            .show(ui, |ui| match &self.transport_state {
                TransportUiState::Ready {
                    node_name,
                    node_fingerprint,
                    local_port,
                } => {
                    ui.label(RichText::new(node_name).size(18.0).strong());
                    ui.label(
                        RichText::new(format!("127.0.0.1:{local_port}"))
                            .color(Color32::from_rgb(99, 220, 160)),
                    );
                    ui.label(
                        RichText::new(node_fingerprint)
                            .size(11.0)
                            .color(Color32::from_gray(120)),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(ui_text(self.language, UiMessage::TransportOnlyWarning))
                            .color(Color32::from_rgb(238, 187, 88)),
                    );
                }
                _ => {
                    ui.label(
                        RichText::new(ui_text(self.language, UiMessage::NoActiveRoutes))
                            .color(Color32::from_gray(150)),
                    );
                }
            });
    }

    fn subscriptions(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new(ui_text(self.language, UiMessage::Subscriptions)).size(30.0));
        ui.label(
            RichText::new(ui_text(self.language, UiMessage::SubscriptionContent))
                .color(Color32::from_gray(150)),
        );
        ui.add(
            egui::TextEdit::multiline(&mut self.subscription_input)
                .desired_rows(7)
                .hint_text("vless://…\ntrojan://…\nss://…\nhysteria2://…"),
        );

        ui.horizontal(|ui| {
            let import_label = ui_text(self.language, UiMessage::Import);
            if brand_button(
                ui,
                egui::include_image!("../../../assets/brand/add-button.svg"),
                44.0,
                import_label,
            )
            .clicked()
            {
                self.import_subscription_text();
            }
            ui.label(RichText::new(import_label).strong());
            if self.editing_node.is_some() {
                ui.label(
                    RichText::new("Editing selected node locally")
                        .size(12.0)
                        .color(Color32::from_rgb(115, 225, 240)),
                );
                if brand_button(
                    ui,
                    egui::include_image!("../../../assets/brand/close-button.svg"),
                    36.0,
                    "Cancel editing",
                )
                .clicked()
                {
                    self.cancel_node_edit();
                }
            }
        });

        ui.add_space(12.0);
        ui.label(format!(
            "{}: {}",
            ui_text(self.language, UiMessage::ImportedNodes),
            self.imported_nodes.len()
        ));

        if !self.imported_nodes.is_empty() {
            let selected = self
                .imported_nodes
                .get(self.selected_node)
                .map(|node| node.display_name.as_str())
                .unwrap_or("—");
            let previous_selected = self.selected_node;
            egui::ComboBox::from_id_salt("transport-node")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (index, node) in self.imported_nodes.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.selected_node,
                            index,
                            format!("{} · {:?}", node.display_name, node.protocol),
                        );
                    }
                });
            if self.selected_node != previous_selected {
                self.delete_confirmation = None;
                self.info_node = None;
                self.node_actions_open = false;
            }

            if brand_button(
                ui,
                egui::include_image!("../../../assets/brand/more-button.svg"),
                42.0,
                "More node actions",
            )
            .clicked()
            {
                self.node_actions_open = !self.node_actions_open;
                if !self.node_actions_open {
                    self.delete_confirmation = None;
                    self.info_node = None;
                }
            }

            if self.node_actions_open {
                let (edit_clicked, delete_clicked, copy_clicked, info_clicked) = ui
                    .horizontal(|ui| {
                        let edit = brand_button(
                            ui,
                            egui::include_image!("../../../assets/brand/edit-button.svg"),
                            42.0,
                            "Edit selected node",
                        )
                        .clicked();
                        let delete = brand_button(
                            ui,
                            egui::include_image!("../../../assets/brand/delete-button.svg"),
                            42.0,
                            "Delete selected node",
                        )
                        .clicked();
                        let copy = brand_button(
                            ui,
                            egui::include_image!("../../../assets/brand/copy-button.svg"),
                            42.0,
                            "Copy node fingerprint",
                        )
                        .clicked();
                        let info = brand_button(
                            ui,
                            egui::include_image!("../../../assets/brand/info-button.svg"),
                            42.0,
                            "Show safe node information",
                        )
                        .clicked();
                        (edit, delete, copy, info)
                    })
                    .inner;

                if edit_clicked {
                    self.begin_edit_selected_node();
                }
                if delete_clicked {
                    self.delete_confirmation = Some(self.selected_node);
                    self.info_node = None;
                }
                if copy_clicked {
                    if let Some(node) = self.imported_nodes.get(self.selected_node) {
                        ui.ctx().copy_text(node.fingerprint.clone());
                    }
                }
                if info_clicked {
                    self.delete_confirmation = None;
                    self.info_node = if self.info_node == Some(self.selected_node) {
                        None
                    } else {
                        Some(self.selected_node)
                    };
                }

                if self.info_node == Some(self.selected_node) {
                    if let Some(node) = self.imported_nodes.get(self.selected_node) {
                        let host = node.host.as_deref().unwrap_or("—");
                        let port = node
                            .port
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "—".into());
                        egui::Frame::new()
                            .fill(Color32::from_rgb(18, 35, 51))
                            .stroke(Stroke::new(1.0, Color32::from_rgb(46, 132, 177)))
                            .corner_radius(14)
                            .inner_margin(14)
                            .show(ui, |ui| {
                                ui.label(RichText::new(&node.display_name).size(15.0).strong());
                                ui.label(format!("Protocol: {:?}", node.protocol));
                                ui.label(format!("Host: {host}"));
                                ui.label(format!("Port: {port}"));
                                ui.label(
                                    RichText::new(format!("Fingerprint: {}", node.fingerprint))
                                        .monospace()
                                        .size(10.0)
                                        .color(Color32::from_rgb(145, 218, 240)),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(
                                        "Secret URI and credentials are intentionally hidden.",
                                    )
                                    .size(11.0)
                                    .color(Color32::from_gray(155)),
                                );
                            });
                    }
                }

                if self.delete_confirmation == Some(self.selected_node) {
                    let safe_name = self
                        .imported_nodes
                        .get(self.selected_node)
                        .map(|node| node.display_name.clone())
                        .unwrap_or_else(|| "selected node".into());
                    let mut confirm = false;
                    let mut cancel = false;
                    egui::Frame::new()
                        .fill(Color32::from_rgb(45, 28, 34))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(154, 76, 91)))
                        .corner_radius(14)
                        .inner_margin(14)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("Delete ‘{safe_name}’?"))
                                    .strong()
                                    .color(Color32::from_rgb(255, 218, 224)),
                            );
                            ui.label(
                                RichText::new("The credential-bearing URI is not shown here.")
                                    .size(11.0)
                                    .color(Color32::from_gray(155)),
                            );
                            ui.horizontal(|ui| {
                                confirm = ui
                                    .add(
                                        egui::Button::new("Delete")
                                            .fill(Color32::from_rgb(118, 43, 58))
                                            .corner_radius(10),
                                    )
                                    .clicked();
                                cancel = ui
                                    .add(egui::Button::new("Cancel").corner_radius(10))
                                    .clicked();
                            });
                        });
                    if confirm {
                        self.delete_confirmed_node();
                    } else if cancel {
                        self.delete_confirmation = None;
                    }
                }
            }
        }

        ui.add_space(12.0);
        ui.label(ui_text(self.language, UiMessage::CoreExecutable));
        ui.text_edit_singleline(&mut self.core_path);
        ui.label(ui_text(self.language, UiMessage::LocalPort));
        ui.text_edit_singleline(&mut self.local_port);
        ui.label(
            RichText::new(ui_text(self.language, UiMessage::TransportOnlyWarning))
                .size(12.0)
                .color(Color32::from_rgb(238, 187, 88)),
        );
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new(ui_text(self.language, UiMessage::Settings)).size(30.0));
        ui.add_space(12.0);
        Self::toggle_row(
            ui,
            ui_text(self.language, UiMessage::SmartRouting),
            ui_text(self.language, UiMessage::SmartRoutingDescription),
            &mut self.smart_routing,
        );
        Self::toggle_row(
            ui,
            ui_text(self.language, UiMessage::KillSwitch),
            ui_text(self.language, UiMessage::KillSwitchDescription),
            &mut self.kill_switch,
        );
        Self::toggle_row(
            ui,
            ui_text(self.language, UiMessage::LocalLearning),
            ui_text(self.language, UiMessage::LocalLearningDescription),
            &mut self.learning,
        );
        Self::toggle_row(
            ui,
            ui_text(self.language, UiMessage::FederatedLearning),
            ui_text(self.language, UiMessage::FederatedDescription),
            &mut self.federated_learning,
        );
        Self::toggle_row(
            ui,
            ui_text(self.language, UiMessage::BackgroundTesting),
            ui_text(self.language, UiMessage::BackgroundDescription),
            &mut self.background_probing,
        );
        ui.add_space(16.0);
        ui.label(ui_text(self.language, UiMessage::CoreExecutable));
        ui.text_edit_singleline(&mut self.core_path);
        ui.label(ui_text(self.language, UiMessage::LocalPort));
        ui.text_edit_singleline(&mut self.local_port);
    }

    fn placeholder(&self, ui: &mut egui::Ui, title: &str) {
        ui.heading(RichText::new(title).size(30.0));
        ui.label(
            RichText::new(ui_text(self.language, UiMessage::NoActiveRoutes))
                .color(Color32::from_gray(150)),
        );
    }
}

impl eframe::App for AmriApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.refresh_transport_state(ui.ctx());
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(3, 8, 18)))
            .show(ui, |ui| {
                desktop_background(ui);
                ui.horizontal(|ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgba_premultiplied(16, 20, 27, 238))
                        .corner_radius(22)
                        .inner_margin(16)
                        .show(ui, |ui| {
                            ui.set_min_height(ui.available_height());
                            self.sidebar(ui);
                        });

                    ui.add_space(10.0);

                    egui::Frame::new()
                        .fill(Color32::from_rgba_premultiplied(13, 16, 22, 224))
                        .corner_radius(22)
                        .inner_margin(24)
                        .show(ui, |ui| {
                            ui.set_min_height(ui.available_height());
                            ui.set_min_width((ui.available_width() - 8.0).max(600.0));
                            if self.page != Page::Home {
                                if brand_button(
                                    ui,
                                    egui::include_image!("../../../assets/brand/back-button.svg"),
                                    36.0,
                                    "Back",
                                )
                                .clicked()
                                {
                                    self.navigate_back();
                                }
                                ui.add_space(8.0);
                            }
                            match self.page {
                                Page::Home => self.home(ui),
                                Page::Routes => self.routes(ui),
                                Page::Subscriptions => self.subscriptions(ui),
                                Page::Rules => {
                                    self.placeholder(ui, ui_text(self.language, UiMessage::Rules))
                                }
                                Page::Settings => self.settings(ui),
                            }
                        });
                });
            });
    }
}
