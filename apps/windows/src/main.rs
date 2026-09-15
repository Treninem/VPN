#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod transport_worker;

use amri_core::{ui_text, Language, UiMessage};
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
    language: Language,
    transport: TransportWorker,
    transport_state: TransportUiState,
    subscription_input: String,
    imported_nodes: Vec<ImportedNode>,
    selected_node: usize,
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
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(13, 16, 22);
        visuals.window_fill = Color32::from_rgb(19, 23, 31);
        visuals.extreme_bg_color = Color32::from_rgb(10, 12, 17);
        visuals.faint_bg_color = Color32::from_rgb(24, 29, 39);
        visuals.selection.bg_fill = Color32::from_rgb(67, 104, 255);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(12);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(12);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(12);
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(12);
        cc.egui_ctx.set_visuals(visuals);

        let mut style = (*cc.egui_ctx.style_of(egui::Theme::Dark)).clone();
        style.spacing.item_spacing = Vec2::new(12.0, 12.0);
        style.spacing.button_padding = Vec2::new(16.0, 10.0);
        cc.egui_ctx.set_style_of(egui::Theme::Dark, style);

        let language = std::env::var("LANG")
            .map(|tag| Language::from_tag(&tag))
            .unwrap_or(Language::English);

        Self {
            page: Page::Home,
            language,
            transport: TransportWorker::new(),
            transport_state: TransportUiState::Idle,
            subscription_input: String::new(),
            imported_nodes: Vec::new(),
            selected_node: 0,
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

    fn refresh_transport_state(&mut self, ctx: &egui::Context) {
        if let Some(state) = self.transport.latest_state() {
            self.transport_state = state;
        }
        if matches!(&self.transport_state, TransportUiState::Connecting) {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    fn import_subscription_text(&mut self) {
        self.imported_nodes = parse_subscription_text("windows-manual", &self.subscription_input);
        self.selected_node = 0;
        if self.imported_nodes.is_empty() {
            self.transport_state =
                TransportUiState::Failed("no supported VPN nodes were found".into());
        } else if matches!(&self.transport_state, TransportUiState::Failed(_)) {
            self.transport_state = TransportUiState::Idle;
        }
    }

    fn start_transport(&mut self) {
        let Some(node) = self.imported_nodes.get(self.selected_node).cloned() else {
            self.page = Page::Subscriptions;
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
        ui.label(
            RichText::new(ui_text(self.language, UiMessage::LanguageLabel))
                .size(11.0)
                .color(Color32::from_gray(125)),
        );
        egui::ComboBox::from_id_salt("ui-language")
            .selected_text(self.language.native_name())
            .show_ui(ui, |ui| {
                for language in Language::ALL {
                    ui.selectable_value(&mut self.language, language, language.native_name());
                }
            });
        ui.add_space(26.0);

        let language = self.language;
        self.nav_button(ui, Page::Home, ui_text(language, UiMessage::Home));
        self.nav_button(ui, Page::Routes, ui_text(language, UiMessage::Routes));
        self.nav_button(
            ui,
            Page::Subscriptions,
            ui_text(language, UiMessage::Subscriptions),
        );
        self.nav_button(ui, Page::Rules, ui_text(language, UiMessage::Rules));
        self.nav_button(ui, Page::Settings, ui_text(language, UiMessage::Settings));

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
            self.page = page;
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
            .fill(Color32::from_rgb(22, 27, 36))
            .corner_radius(18)
            .inner_margin(18)
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
            .fill(Color32::from_rgb(24, 31, 44))
            .corner_radius(24)
            .inner_margin(24)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(ui_text(self.language, UiMessage::ProtectionOff))
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
                        let fill = if self.transport_ready() {
                            Color32::from_rgb(58, 72, 98)
                        } else {
                            Color32::from_rgb(67, 104, 255)
                        };
                        let button = egui::Button::new(RichText::new(label).size(16.0).strong())
                            .fill(fill)
                            .stroke(Stroke::NONE)
                            .corner_radius(18);
                        let enabled =
                            !matches!(&self.transport_state, TransportUiState::Connecting);
                        if ui.add_enabled_ui(enabled, |ui| {
                            ui.add_sized([150.0, 54.0], button)
                        }).inner.clicked()
                        {
                            if self.transport_ready() {
                                self.stop_transport();
                            } else if self.imported_nodes.is_empty() {
                                self.page = Page::Subscriptions;
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
            Self::metric_card(
                ui,
                ui_text(self.language, UiMessage::AveragePing),
                "—",
                "",
            );
            Self::metric_card(
                ui,
                ui_text(self.language, UiMessage::Jitter),
                "—",
                "",
            );
            Self::metric_card(
                ui,
                ui_text(self.language, UiMessage::RouteScore),
                "—",
                "",
            );
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
                        RichText::new(ui_text(
                            self.language,
                            UiMessage::TransportOnlyWarning,
                        ))
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
        ui.heading(
            RichText::new(ui_text(self.language, UiMessage::Subscriptions)).size(30.0),
        );
        ui.label(
            RichText::new(ui_text(
                self.language,
                UiMessage::SubscriptionContent,
            ))
            .color(Color32::from_gray(150)),
        );
        ui.add(
            egui::TextEdit::multiline(&mut self.subscription_input)
                .desired_rows(7)
                .hint_text("vless://…\ntrojan://…\nss://…\nhysteria2://…"),
        );

        if ui
            .add(
                egui::Button::new(ui_text(self.language, UiMessage::Import))
                    .corner_radius(12),
            )
            .clicked()
        {
            self.import_subscription_text();
        }

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
        }

        ui.add_space(12.0);
        ui.label(ui_text(self.language, UiMessage::CoreExecutable));
        ui.text_edit_singleline(&mut self.core_path);
        ui.label(ui_text(self.language, UiMessage::LocalPort));
        ui.text_edit_singleline(&mut self.local_port);
        ui.label(
            RichText::new(ui_text(
                self.language,
                UiMessage::TransportOnlyWarning,
            ))
            .size(12.0)
            .color(Color32::from_rgb(238, 187, 88)),
        );
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
            .frame(egui::Frame::new().fill(Color32::from_rgb(13, 16, 22)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(16, 20, 27))
                        .inner_margin(16)
                        .show(ui, |ui| {
                            ui.set_min_height(ui.available_height());
                            self.sidebar(ui);
                        });

                    ui.add_space(10.0);

                    egui::Frame::new()
                        .fill(Color32::from_rgb(13, 16, 22))
                        .inner_margin(24)
                        .show(ui, |ui| {
                            ui.set_min_height(ui.available_height());
                            ui.set_min_width((ui.available_width() - 8.0).max(600.0));
                            match self.page {
                                Page::Home => self.home(ui),
                                Page::Routes => self.routes(ui),
                                Page::Subscriptions => self.subscriptions(ui),
                                Page::Rules => {
                                    self.placeholder(ui, ui_text(self.language, UiMessage::Rules))
                                }
                                Page::Settings => self
                                    .placeholder(ui, ui_text(self.language, UiMessage::Settings)),
                            }
                        });
                });
            });
    }
}
