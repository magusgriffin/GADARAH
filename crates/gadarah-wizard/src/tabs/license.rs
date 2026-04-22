use eframe::egui::{self, RichText, ScrollArea};

use crate::theme;

const LICENSE_TEXT: &str = "\
GADARAH is dual-licensed under MIT OR Apache-2.0.

You may choose either license at your option. Both licenses grant permissive \
rights to use, modify, and redistribute the software, including in commercial \
and proprietary products.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND. TRADING \
FINANCIAL INSTRUMENTS INVOLVES SUBSTANTIAL RISK OF LOSS. THE AUTHORS ACCEPT \
NO RESPONSIBILITY FOR LOSSES INCURRED THROUGH USE OF THIS SOFTWARE.

The full text of both licenses will be installed alongside the application \
at %LOCALAPPDATA%\\GADARAH\\LICENSE-MIT and LICENSE-APACHE.

By installing, you confirm that:
 • You have read and accepted the applicable license.
 • You understand that this is a tool, not financial advice.
 • You will verify all trading behaviour on a paper account before going live.
";

pub fn show(ui: &mut egui::Ui, accepted: &mut bool) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("License Agreement")
                .heading()
                .color(theme::FORGE_GOLD),
        );
        ui.add_space(8.0);
        ScrollArea::vertical()
            .auto_shrink([false; 2])
            .max_height(260.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(LICENSE_TEXT)
                        .color(theme::TEXT)
                        .size(12.5),
                );
            });
        ui.add_space(10.0);
        ui.checkbox(
            accepted,
            "I have read and accept the license terms (MIT OR Apache-2.0).",
        );
    });
}
