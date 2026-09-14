#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use amri_core::ShadowRacePolicy;
use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Stroke, Vec2};

const BACKGROUND: egui::ImageSource<'static> =
    egui::include_image!("../../../assets/brand/background-desktop.svg");
const POWER_OFF: egui::ImageSource<'static> =
    egui::include_image!("../../../assets/brand/vpn-power-off.svg");
const POWER_ON: egui::ImageSource<'static> =
    egui::include_image!("../../../assets/brand/vpn-power-on.svg");

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AMRI VPN")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([980.0, 680.0]),
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
    transport_active: bool,
    notice: Option<String>,
    smart_routing: bool,
    kill_switch: bool,
    learning: bool,
    federated_learning: bool,
    background_probing: bool,
    shadow_policy: ShadowRacePolicy,
}

impl AmriApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::TRANSPARENT;
        visuals.window_fill = Color32::from_rgb(7, 16, 35);
        visuals.extreme_bg_color = Color32::from_rgb(2, 7, 19);
        visuals.faint_bg_color = Color32::from_rgba_unmultiplied(15, 35, 72, 210);
        visuals.selection.bg_fill = Color32::from_rgb(18, 151, 233);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(12);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(12);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(12);
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(12);
        cc.egui_ctx.set_visuals(visuals);

        let mut style = (*cc.egui_ctx.style_of(egui::Theme::Dark)).clone();
        style.spacing.item_spacing = Vec2::new(12.0, 12.0);
        style.spacing.button_padding = Vec2::new(16.0, 10.0);
        cc.egui_ctx.set_style_of(egui::Theme::Dark, style);

        Self {
            page: Page::Home,
            transport_active: false,
            notice: None,
            smart_routing: true,
            kill_switch: true,
            learning: true,
            federated_learning: false,
            background_probing: true,
            shadow_policy: ShadowRacePolicy::default(),
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.set_width(220.0);
        ui.add_space(8.0);
        ui.label(RichText::new("AMRI").size(28.0).strong());
        ui.label(RichText::new("Adaptive VPN").color(Color32::from_gray(175)));
        ui.add_space(26.0);

        self.nav_button(ui, Page::Home, "⌂  Главная");
        self.nav_button(ui, Page::Routes, "↗  Маршруты");
        self.nav_button(ui, Page::Subscriptions, "⊕  Подписки");
        self.nav_button(ui, Page::Rules, "◎  Правила");
        self.nav_button(ui, Page::Settings, "⚙  Настройки");

        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.add_space(8.0);
            ui.label(
                RichText::new("Local Intelligence Engine")
                    .size(12.0)
                    .color(Color32::from_gray(150)),
            );
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                ui.painter()
                    .circle_filled(rect.center(), 4.0, Color32::from_rgb(47, 207, 154));
                ui.label(RichText::new("AMRI готов к измерениям").size(12.0));
            });
        });
    }

    fn nav_button(&mut self, ui: &mut egui::Ui, page: Page, text: &str) {
        let selected = self.page == page;
        let fill = if selected {
            Color32::from_rgba_unmultiplied(18, 105, 205, 170)
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
            .fill(Color32::from_rgba_unmultiplied(10, 25, 52, 218))
            .corner_radius(16)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(label).size(15.0).strong());
                        ui.label(
                            RichText::new(description)
                                .size(12.0)
                                .color(Color32::from_gray(170)),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let state_text = if *value { "Вкл" } else { "Выкл" };
                        ui.toggle_value(value, state_text);
                    });
                });
            });
    }

    fn metric_card(ui: &mut egui::Ui, title: &str, value: &str, subtitle: &str) {
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(10, 25, 52, 218))
            .corner_radius(18)
            .inner_margin(18)
            .show(ui, |ui| {
                ui.set_min_width(180.0);
                ui.label(
                    RichText::new(title)
                        .size(12.0)
                        .color(Color32::from_gray(175)),
                );
                ui.add_space(6.0);
                ui.label(RichText::new(value).size(26.0).strong());
                ui.add_space(2.0);
                ui.label(
                    RichText::new(subtitle)
                        .size(12.0)
                        .color(Color32::from_gray(150)),
                );
            });
    }

    fn power_control(&mut self, ui: &mut egui::Ui) {
        let source = if self.transport_active {
            POWER_ON
        } else {
            POWER_OFF
        };
        let label = if self.transport_active {
            "Отключить защищённый transport"
        } else {
            "Подключить AMRI VPN"
        };
        let image = egui::Image::new(source)
            .fit_to_exact_size(Vec2::splat(136.0))
            .alt_text(label)
            .sense(Sense::click());
        let response = ui.add(image).on_hover_text(label);

        if response.clicked() {
            if self.transport_active {
                self.transport_active = false;
                self.notice = Some("Защищённый transport остановлен.".into());
            } else {
                self.page = Page::Subscriptions;
                self.notice = Some(
                    "Сначала добавьте подписку. AMRI не показывает ложный статус защиты без работающего transport и packet forwarding."
                        .into(),
                );
            }
        }
        ui.label(RichText::new(label).size(14.0).strong());
    }


    fn route_galaxy(&mut self, ui: &mut egui::Ui) {
        let size = Vec2::new(ui.available_width().min(820.0), 252.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(
            rect,
            24.0,
            Color32::from_rgba_unmultiplied(4, 18, 43, 232),
        );
        painter.rect_stroke(
            rect,
            24.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(34, 202, 242, 120)),
            egui::StrokeKind::Inside,
        );

        let center = rect.center();
        for radius in [54.0, 91.0, 119.0] {
            painter.circle_stroke(
                center,
                radius,
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(58, 170, 239, 55)),
            );
        }
        painter.circle_filled(center, 39.0, Color32::from_rgb(8, 91, 178));
        painter.circle_stroke(
            center,
            39.0,
            Stroke::new(2.0, Color32::from_rgb(58, 225, 242)),
        );
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            "AMRI",
            egui::FontId::proportional(16.0),
            Color32::WHITE,
        );

        let nodes = [
            (Vec2::new(-164.0, -57.0), "ПУЛ", "0 узлов"),
            (Vec2::new(168.0, -48.0), "PROBE", "ожидание"),
            (Vec2::new(-150.0, 73.0), "ROUTES", "0 активно"),
            (Vec2::new(165.0, 72.0), "DIRECT", "готов"),
        ];
        for (offset, title, value) in nodes {
            let point = center + offset;
            painter.line_segment(
                [center, point],
                Stroke::new(1.5, Color32::from_rgba_unmultiplied(37, 186, 239, 95)),
            );
            painter.circle_filled(point, 25.0, Color32::from_rgb(9, 36, 72));
            painter.circle_stroke(
                point,
                25.0,
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(64, 202, 242, 150)),
            );
            painter.text(
                point + Vec2::new(0.0, -4.0),
                egui::Align2::CENTER_CENTER,
                title,
                egui::FontId::proportional(11.0),
                Color32::WHITE,
            );
            painter.text(
                point + Vec2::new(0.0, 11.0),
                egui::Align2::CENTER_CENTER,
                value,
                egui::FontId::proportional(9.0),
                Color32::from_gray(170),
            );
        }
        painter.text(
            rect.left_top() + Vec2::new(20.0, 18.0),
            egui::Align2::LEFT_TOP,
            "AMRI ROUTE GALAXY",
            egui::FontId::proportional(13.0),
            Color32::from_rgb(65, 221, 239),
        );
        painter.text(
            rect.left_bottom() + Vec2::new(20.0, -18.0),
            egui::Align2::LEFT_BOTTOM,
            "Живая карта: назначение → маршрут → VPN-выход / DIRECT",
            egui::FontId::proportional(11.0),
            Color32::from_gray(170),
        );

        let response = response.on_hover_text(
            "Открыть живую карту маршрутов. Лучи появятся только для реальных соединений.",
        );
        if response.clicked() {
            self.page = Page::Routes;
        }
    }

    fn shadow_race_card(&self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(8, 32, 66, 232))
            .stroke(Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(30, 195, 242, 110),
            ))
            .corner_radius(20)
            .inner_margin(18)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Теневая гонка AMRI").size(18.0).strong());
                        ui.label(
                            RichText::new(
                                "Параллельно подтверждает новый маршрут и не прерывает текущий из-за случайного скачка score.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(180)),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("ОЖИДАНИЕ МАРШРУТОВ")
                                .size(11.0)
                                .strong()
                                .color(Color32::from_rgb(65, 221, 239)),
                        );
                    });
                });
                ui.add_space(10.0);
                ui.label(
                    RichText::new(format!(
                        "Мгновенный burst: {} параллельных подтверждения · ≥{:.0}% улучшения · confidence ≥{:.0}%",
                        self.shadow_policy.required_wins,
                        self.shadow_policy.min_improvement_percent,
                        self.shadow_policy.min_confidence
                    ))
                    .size(12.0)
                    .color(Color32::from_gray(165)),
                );
            });
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Главная").size(30.0));
        ui.label(
            RichText::new("Лучший маршрут для каждого соединения — автоматически")
                .color(Color32::from_gray(185)),
        );
        ui.add_space(18.0);
        self.route_galaxy(ui);
        ui.add_space(18.0);

        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(8, 27, 59, 230))
            .stroke(Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(39, 163, 239, 100),
            ))
            .corner_radius(24)
            .inner_margin(24)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(if self.transport_active {
                                "Защита включена"
                            } else {
                                "Защита выключена"
                            })
                            .size(24.0)
                            .strong(),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(if self.transport_active {
                                "Transport и packet forwarding подтверждены"
                            } else {
                                "Добавьте подписку, чтобы подготовить первое реальное подключение"
                            })
                            .color(Color32::from_gray(180)),
                        );
                        if let Some(notice) = &self.notice {
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new(notice)
                                    .size(12.0)
                                    .color(Color32::from_rgb(113, 213, 255)),
                            );
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.vertical_centered(|ui| self.power_control(ui));
                    });
                });
            });

        ui.add_space(18.0);
        ui.horizontal_wrapped(|ui| {
            Self::metric_card(ui, "Активные маршруты", "0", "ожидается transport");
            Self::metric_card(ui, "Средний ping", "—", "нет реальных проб");
            Self::metric_card(ui, "Jitter", "—", "нет реальных проб");
            Self::metric_card(ui, "RouteScore", "—", "нет выбранного маршрута");
        });

        ui.add_space(18.0);
        self.shadow_race_card(ui);
        ui.add_space(20.0);
        ui.label(RichText::new("Умная маршрутизация").size(20.0).strong());
        ui.add_space(8.0);

        Self::toggle_row(
            ui,
            "Smart Routing",
            "Выбирать сервер отдельно для каждого сайта и приложения",
            &mut self.smart_routing,
        );
        Self::toggle_row(
            ui,
            "Локальное обучение",
            "Запоминать лучшие маршруты только на этом компьютере",
            &mut self.learning,
        );
        Self::toggle_row(
            ui,
            "Обмен обезличенным опытом",
            "Передавать только агрегаты без истории сайтов и личных данных",
            &mut self.federated_learning,
        );
        Self::toggle_row(
            ui,
            "Фоновое тестирование",
            "Проверять кандидатов для Теневой гонки без вмешательства пользователя",
            &mut self.background_probing,
        );
        Self::toggle_row(
            ui,
            "Kill Switch",
            "Блокировать защищённый трафик при потере VPN-маршрута",
            &mut self.kill_switch,
        );
    }

    fn routes(&self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Маршруты").size(30.0));
        ui.label(
            RichText::new("Текущие назначения, выбранные узлы и объяснения решений")
                .color(Color32::from_gray(185)),
        );
        ui.add_space(18.0);
        self.shadow_race_card(ui);
        ui.add_space(14.0);
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(10, 25, 52, 218))
            .corner_radius(18)
            .inner_margin(22)
            .show(ui, |ui| {
                ui.label(RichText::new("Активных маршрутов пока нет").size(16.0).strong());
                ui.label(
                    RichText::new(
                        "Демонстрационные серверы удалены: здесь появятся только реальные данные transport и probes.",
                    )
                    .size(12.0)
                    .color(Color32::from_gray(170)),
                );
            });
    }

    fn placeholder(&self, ui: &mut egui::Ui, title: &str, subtitle: &str) {
        ui.heading(RichText::new(title).size(30.0));
        ui.label(RichText::new(subtitle).color(Color32::from_gray(185)));
    }
}

impl eframe::App for AmriApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(2, 7, 19)))
            .show(ui, |ui| {
                egui::Image::new(BACKGROUND)
                    .fit_to_exact_size(ui.available_size())
                    .paint_at(ui, ui.max_rect());

                ui.horizontal(|ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(3, 12, 29, 220))
                        .inner_margin(16)
                        .show(ui, |ui| {
                            ui.set_min_height(ui.available_height());
                            self.sidebar(ui);
                        });

                    ui.add_space(10.0);

                    egui::Frame::new()
                        .fill(Color32::TRANSPARENT)
                        .inner_margin(24)
                        .show(ui, |ui| {
                            ui.set_min_height(ui.available_height());
                            ui.set_min_width((ui.available_width() - 8.0).max(600.0));
                            match self.page {
                                Page::Home => self.home(ui),
                                Page::Routes => self.routes(ui),
                                Page::Subscriptions => self.placeholder(
                                    ui,
                                    "Подписки",
                                    "Добавление нескольких подписок, единый пул узлов и приоритеты.",
                                ),
                                Page::Rules => self.placeholder(
                                    ui,
                                    "Правила",
                                    "DIRECT / VPN / BLOCK, приложения, домены и резервные маршруты.",
                                ),
                                Page::Settings => self.placeholder(
                                    ui,
                                    "Настройки",
                                    "Темы, DNS, автозапуск, обучение, федеративный обмен и приватность.",
                                ),
                            }
                        });
                });
            });
    }
}
