#!/usr/bin/env python3
from pathlib import Path

path = Path("apps/windows/src/main.rs")
text = path.read_text(encoding="utf-8")

old_brand = '''        ui.add_space(8.0);
        ui.label(RichText::new("AMRI").size(28.0).strong());
        ui.label(
            RichText::new(ui_text(self.language, UiMessage::AdaptiveVpn))
                .color(Color32::from_gray(145)),
        );
        ui.add_space(12.0);
'''

new_brand = '''        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(egui::include_image!(
                    "../../../assets/brand/amri-icon.png"
                ))
                .fit_to_exact_size(Vec2::splat(62.0))
                .alt_text("AMRI VPN"),
            );
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new("AMRI VPN").size(24.0).strong());
                ui.label(
                    RichText::new(ui_text(self.language, UiMessage::AdaptiveVpn))
                        .size(11.0)
                        .color(Color32::from_gray(145)),
                );
            });
        });
        ui.add_space(14.0);
'''

if old_brand not in text:
    raise SystemExit("brand anchor not found")
text = text.replace(old_brand, new_brand, 1)

marker = "impl eframe::App for AmriApp {"
head, sep, _tail = text.partition(marker)
if not sep:
    raise SystemExit("app implementation anchor not found")

new_app = r'''impl eframe::App for AmriApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.refresh_transport_state(ui.ctx());
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(3, 8, 18)))
            .show(ui, |ui| {
                desktop_background(ui);

                ui.horizontal_top(|ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgba_premultiplied(16, 20, 27, 242))
                        .stroke(Stroke::new(
                            1.0,
                            Color32::from_rgba_premultiplied(67, 104, 255, 42),
                        ))
                        .corner_radius(22)
                        .inner_margin(16)
                        .show(ui, |ui| {
                            ui.set_width(236.0);
                            ui.set_min_height(ui.available_height());
                            ui.vertical(|ui| {
                                ui.set_width(220.0);
                                self.sidebar(ui);
                            });
                        });

                    ui.add_space(10.0);

                    egui::Frame::new()
                        .fill(Color32::from_rgba_premultiplied(13, 16, 22, 232))
                        .stroke(Stroke::new(
                            1.0,
                            Color32::from_rgba_premultiplied(67, 104, 255, 34),
                        ))
                        .corner_radius(22)
                        .inner_margin(24)
                        .show(ui, |ui| {
                            let content_width = ui.available_width().max(600.0);
                            ui.set_min_width(content_width);
                            ui.set_min_height(ui.available_height());

                            ui.vertical(|ui| {
                                if self.page != Page::Home {
                                    ui.horizontal(|ui| {
                                        if brand_button(
                                            ui,
                                            egui::include_image!(
                                                "../../../assets/brand/back-button.svg"
                                            ),
                                            36.0,
                                            "Back",
                                        )
                                        .clicked()
                                        {
                                            self.navigate_back();
                                        }
                                    });
                                    ui.add_space(8.0);
                                }

                                egui::ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        match self.page {
                                            Page::Home => self.home(ui),
                                            Page::Routes => self.routes(ui),
                                            Page::Subscriptions => self.subscriptions(ui),
                                            Page::Rules => self.placeholder(
                                                ui,
                                                ui_text(self.language, UiMessage::Rules),
                                            ),
                                            Page::Settings => self.settings(ui),
                                        }
                                        ui.add_space(24.0);
                                    });
                            });
                        });
                });
            });
    }
}
'''

path.write_text(head + new_app, encoding="utf-8")
