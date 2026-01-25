//! Traffic monitor UI rendering

use std::ops::Range;
use std::time::SystemTime;

use cat_protocol::display::{AnnotatedFrame, FrameSegment};
use egui::{Color32, Id, RichText, Ui};
use tracing::Level;

use super::models::{
    segment_color, DiagnosticSeverity, ExportAction, TrafficDirection, TrafficEntry, TrafficSource,
};
use super::TrafficMonitor;

impl TrafficMonitor {
    /// Draw the traffic monitor UI with display settings
    /// Returns Some(ExportAction) if an export action was requested
    pub fn draw(
        &mut self,
        ui: &mut Ui,
        show_hex: bool,
        show_decoded: bool,
    ) -> Option<ExportAction> {
        let mut export_action = None;
        // Toolbar
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.auto_scroll, "Auto-scroll");
            ui.separator();

            if ui
                .button(if self.paused { "Resume" } else { "Pause" })
                .clicked()
            {
                self.paused = !self.paused;
            }

            if ui.button("Clear").clicked() {
                self.clear();
            }

            // Export dropdown menu
            ui.menu_button("Export", |ui| {
                if ui.button("Copy to Clipboard").clicked() {
                    export_action = Some(ExportAction::CopyToClipboard(self.format_filtered_log()));
                    ui.close_menu();
                }
                if ui.button("Save to File...").clicked() {
                    export_action = Some(match self.save_filtered_log_with_dialog() {
                        Ok(Some(path)) => ExportAction::SavedToFile(path),
                        Ok(None) => ExportAction::Cancelled,
                        Err(e) => ExportAction::Error(e),
                    });
                    ui.close_menu();
                }
            });

            ui.separator();

            // Direction filter
            ui.label("Filter:");
            if ui
                .selectable_label(self.filter_direction.is_none(), "All")
                .clicked()
            {
                self.filter_direction = None;
            }
            if ui
                .selectable_label(
                    self.filter_direction == Some(TrafficDirection::Incoming),
                    "In",
                )
                .clicked()
            {
                self.filter_direction = Some(TrafficDirection::Incoming);
            }
            if ui
                .selectable_label(
                    self.filter_direction == Some(TrafficDirection::Outgoing),
                    "Out",
                )
                .clicked()
            {
                self.filter_direction = Some(TrafficDirection::Outgoing);
            }

            ui.separator();

            // Simulated traffic toggle
            ui.checkbox(&mut self.show_simulated, "Show SIM");

            ui.separator();

            // Diagnostic level selector with dropdown menu
            let level_label = match self.diagnostic_level {
                Some(Level::DEBUG) | Some(Level::TRACE) => "Logs: Debug",
                Some(Level::INFO) => "Logs: Info",
                Some(Level::WARN) => "Logs: Warn",
                Some(Level::ERROR) => "Logs: Error",
                None => "Logs: Off",
            };
            ui.menu_button(
                RichText::new(format!("{} ▾", level_label)).color(
                    if self.diagnostic_level.is_some() {
                        Color32::WHITE
                    } else {
                        Color32::GRAY
                    },
                ),
                |ui| {
                    if ui
                        .selectable_label(self.diagnostic_level.is_none(), "Off")
                        .clicked()
                    {
                        self.diagnostic_level = None;
                        ui.close_menu();
                    }
                    if ui
                        .selectable_label(self.diagnostic_level == Some(Level::ERROR), "Error")
                        .clicked()
                    {
                        self.diagnostic_level = Some(Level::ERROR);
                        ui.close_menu();
                    }
                    if ui
                        .selectable_label(self.diagnostic_level == Some(Level::WARN), "Warning")
                        .clicked()
                    {
                        self.diagnostic_level = Some(Level::WARN);
                        ui.close_menu();
                    }
                    if ui
                        .selectable_label(self.diagnostic_level == Some(Level::INFO), "Info")
                        .clicked()
                    {
                        self.diagnostic_level = Some(Level::INFO);
                        ui.close_menu();
                    }
                    if ui
                        .selectable_label(
                            matches!(
                                self.diagnostic_level,
                                Some(Level::DEBUG) | Some(Level::TRACE)
                            ),
                            "Debug",
                        )
                        .clicked()
                    {
                        self.diagnostic_level = Some(Level::DEBUG);
                        ui.close_menu();
                    }
                },
            );
        });

        ui.separator();

        // Traffic list - collect filtered indices first for proper virtual scrolling
        let filtered_indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| self.entry_passes_filter(entry))
            .map(|(i, _)| i)
            .collect();

        let text_style = egui::TextStyle::Monospace;
        let row_height = ui.text_style_height(&text_style);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(self.auto_scroll)
            .show_rows(ui, row_height, filtered_indices.len(), |ui, row_range| {
                for i in row_range {
                    if let Some(&entry_idx) = filtered_indices.get(i) {
                        if let Some(entry) = self.entries.get(entry_idx) {
                            self.draw_entry(ui, entry, entry_idx, show_hex, show_decoded);
                        }
                    }
                }
                // Bottom padding to prevent scroll jitter at boundary
                ui.add_space(4.0);
            });

        export_action
    }

    /// Draw a single traffic entry
    fn draw_entry(
        &self,
        ui: &mut Ui,
        entry: &TrafficEntry,
        entry_idx: usize,
        show_hex: bool,
        show_decoded: bool,
    ) {
        match entry {
            TrafficEntry::Data {
                timestamp,
                source,
                data,
                decoded,
                ..
            } => {
                self.draw_data_entry(
                    ui,
                    entry_idx,
                    timestamp,
                    source,
                    data,
                    decoded.as_ref(),
                    show_hex,
                    show_decoded,
                );
            }
            TrafficEntry::Diagnostic {
                timestamp,
                source,
                severity,
                message,
            } => {
                self.draw_diagnostic_entry(ui, timestamp, source, severity, message);
            }
        }
    }

    /// Draw a data traffic entry
    #[allow(clippy::too_many_arguments)]
    fn draw_data_entry(
        &self,
        ui: &mut Ui,
        entry_idx: usize,
        timestamp: &SystemTime,
        source: &TrafficSource,
        data: &[u8],
        decoded: Option<&AnnotatedFrame>,
        show_hex: bool,
        show_decoded: bool,
    ) {
        ui.horizontal(|ui| {
            // Create a unique ID for this entry's hover state
            let hover_id = Id::new("traffic_hover").with(entry_idx);

            // Get the currently hovered byte range from previous frame
            let hovered_range: Option<Range<usize>> = ui.memory(|mem| mem.data.get_temp(hover_id));

            // Track new hover state for this frame
            let mut new_hovered_range: Option<Range<usize>> = None;

            // Timestamp
            let time = timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| {
                    let secs = d.as_secs() % 86400;
                    let hours = secs / 3600;
                    let mins = (secs % 3600) / 60;
                    let secs = secs % 60;
                    let millis = d.subsec_millis();
                    format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
                })
                .unwrap_or_default();

            ui.label(RichText::new(time).color(Color32::GRAY).monospace());

            // Simulated badge
            if source.is_simulated() {
                ui.label(
                    RichText::new("[SIM]")
                        .color(Color32::from_rgb(255, 165, 0)) // Orange
                        .strong()
                        .monospace(),
                );
            }

            // Direction indicator with source info
            match source {
                TrafficSource::RealRadio { .. } => {
                    ui.label(
                        RichText::new("[Radio→]")
                            .color(Color32::LIGHT_BLUE)
                            .monospace(),
                    );
                }
                TrafficSource::ToRealRadio { .. } => {
                    ui.label(
                        RichText::new("[→Radio]")
                            .color(Color32::from_rgb(180, 100, 255)) // Purple for outgoing to radio
                            .monospace(),
                    );
                }
                TrafficSource::SimulatedRadio { id } => {
                    ui.label(
                        RichText::new(format!("[{}→]", id))
                            .color(Color32::from_rgb(100, 180, 255))
                            .monospace(),
                    );
                }
                TrafficSource::ToSimulatedRadio { id } => {
                    ui.label(
                        RichText::new(format!("[→{}]", id))
                            .color(Color32::from_rgb(180, 100, 255)) // Purple for outgoing to radio
                            .monospace(),
                    );
                }
                TrafficSource::RealAmplifier { .. } => {
                    ui.label(
                        RichText::new("[→Amp]")
                            .color(Color32::LIGHT_GREEN)
                            .monospace(),
                    );
                }
                TrafficSource::FromRealAmplifier { .. } => {
                    ui.label(
                        RichText::new("[Amp→]")
                            .color(Color32::LIGHT_GREEN)
                            .monospace(),
                    );
                }
                TrafficSource::SimulatedAmplifier => {
                    ui.label(
                        RichText::new("[→Amp]")
                            .color(Color32::from_rgb(100, 180, 100))
                            .monospace(),
                    );
                }
                TrafficSource::FromSimulatedAmplifier => {
                    ui.label(
                        RichText::new("[Amp→]")
                            .color(Color32::from_rgb(100, 180, 100))
                            .monospace(),
                    );
                }
            }

            // Protocol badge
            if let Some(decoded) = decoded {
                let protocol_color = match decoded.protocol {
                    "CI-V" => Color32::from_rgb(255, 180, 100),  // Orange
                    "Yaesu" => Color32::from_rgb(100, 200, 255), // Cyan (binary CAT)
                    "Yaesu ASCII" => Color32::from_rgb(80, 180, 230), // Slightly different cyan
                    "Kenwood" => Color32::from_rgb(180, 255, 100), // Lime
                    "Elecraft" => Color32::from_rgb(200, 255, 120), // Light lime
                    "Flex" => Color32::from_rgb(255, 150, 255),  // Magenta
                    _ => Color32::GRAY,
                };
                ui.label(
                    RichText::new(format!("[{}]", decoded.protocol))
                        .color(protocol_color)
                        .strong()
                        .monospace(),
                );
            }

            // Decoded summary with colored parts (shown first, after badges)
            if show_decoded {
                if let Some(decoded) = decoded {
                    let prev_spacing = ui.spacing().item_spacing.x;
                    ui.spacing_mut().item_spacing.x = 0.0;

                    for part in &decoded.summary {
                        let color = segment_color(part.part_type);

                        // Check if this part should be highlighted
                        let is_highlighted = part
                            .range
                            .as_ref()
                            .map(|pr| {
                                hovered_range
                                    .as_ref()
                                    .map(|hr| hr.start == pr.start && hr.end == pr.end)
                                    .unwrap_or(false)
                            })
                            .unwrap_or(false);

                        let text = if is_highlighted {
                            RichText::new(&part.text)
                                .color(Color32::WHITE)
                                .background_color(Color32::from_rgb(60, 60, 80))
                                .monospace()
                        } else {
                            RichText::new(&part.text).color(color).monospace()
                        };

                        let response = ui.label(text);

                        // Track hover on summary parts to highlight hex/ASCII
                        if let Some(range) = &part.range {
                            if response.hovered() {
                                new_hovered_range = Some(range.clone());
                            }
                        }
                    }

                    ui.spacing_mut().item_spacing.x = prev_spacing;
                }
            }

            // ASCII representation with highlighting
            if show_hex {
                ui.add_space(8.0);
                if let Some(decoded) = decoded {
                    self.draw_ascii_with_segments(
                        ui,
                        data,
                        &decoded.segments,
                        hovered_range.as_ref(),
                        &mut new_hovered_range,
                    );
                } else {
                    let ascii: String = data
                        .iter()
                        .map(|&b| {
                            if b.is_ascii_graphic() || b == b' ' {
                                b as char
                            } else {
                                '.'
                            }
                        })
                        .collect();
                    ui.label(RichText::new(ascii).color(Color32::GRAY).monospace());
                }
            }

            // Color-coded hex data with segment annotations (shown last)
            if show_hex {
                ui.add_space(8.0);
                if let Some(decoded) = decoded {
                    self.draw_colored_hex(
                        ui,
                        data,
                        &decoded.segments,
                        hovered_range.as_ref(),
                        &mut new_hovered_range,
                    );
                } else {
                    let hex: String = data
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    ui.label(RichText::new(hex).monospace());
                }
            }

            // Store hover state for next frame
            ui.memory_mut(|mem| {
                if let Some(range) = new_hovered_range {
                    mem.data.insert_temp(hover_id, range);
                } else {
                    mem.data.remove::<Range<usize>>(hover_id);
                }
            });
        });
    }

    /// Draw a diagnostic entry (error or warning)
    fn draw_diagnostic_entry(
        &self,
        ui: &mut Ui,
        timestamp: &SystemTime,
        source: &str,
        severity: &DiagnosticSeverity,
        message: &str,
    ) {
        ui.horizontal(|ui| {
            // Timestamp
            let time = timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| {
                    let secs = d.as_secs() % 86400;
                    let hours = secs / 3600;
                    let mins = (secs % 3600) / 60;
                    let secs = secs % 60;
                    let millis = d.subsec_millis();
                    format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
                })
                .unwrap_or_default();

            ui.label(RichText::new(time).color(Color32::GRAY).monospace());

            // Severity badge and color
            let (badge, color) = match severity {
                DiagnosticSeverity::Debug => ("●", Color32::from_rgb(128, 128, 128)), // Gray
                DiagnosticSeverity::Info => ("ℹ", Color32::from_rgb(100, 180, 255)),  // Blue
                DiagnosticSeverity::Warning => ("⚠", Color32::from_rgb(255, 200, 0)), // Yellow
                DiagnosticSeverity::Error => ("✖", Color32::from_rgb(255, 80, 80)),   // Red
            };

            ui.label(RichText::new(badge).color(color).monospace());
            ui.label(
                RichText::new(format!("[{}]", source))
                    .color(color)
                    .strong()
                    .monospace(),
            );
            ui.label(RichText::new(message).color(color).monospace());
        });
    }

    /// Draw ASCII representation with segment-based highlighting
    fn draw_ascii_with_segments(
        &self,
        ui: &mut Ui,
        data: &[u8],
        segments: &[FrameSegment],
        hovered_range: Option<&Range<usize>>,
        new_hovered_range: &mut Option<Range<usize>>,
    ) {
        let mut sorted_segments: Vec<_> = segments.iter().collect();
        sorted_segments.sort_by_key(|s| s.range.start);

        let prev_spacing = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 0.0;

        let mut pos = 0;
        for seg in &sorted_segments {
            // Handle gap before segment
            while pos < seg.range.start && pos < data.len() {
                let ch = if data[pos].is_ascii_graphic() || data[pos] == b' ' {
                    data[pos] as char
                } else {
                    '.'
                };
                ui.label(RichText::new(ch).color(Color32::DARK_GRAY).monospace());
                pos += 1;
            }

            // Render segment's ASCII
            if seg.range.start < data.len() {
                let end = seg.range.end.min(data.len());
                let ascii: String = data[seg.range.start..end]
                    .iter()
                    .map(|&b| {
                        if b.is_ascii_graphic() || b == b' ' {
                            b as char
                        } else {
                            '.'
                        }
                    })
                    .collect();

                // Check if this segment is highlighted
                let is_highlighted = hovered_range
                    .map(|hr| hr.start == seg.range.start && hr.end == seg.range.end)
                    .unwrap_or(false);

                let color = if is_highlighted {
                    Color32::WHITE
                } else {
                    segment_color(seg.segment_type)
                };

                let text = if is_highlighted {
                    RichText::new(&ascii)
                        .color(color)
                        .background_color(Color32::from_rgb(60, 60, 80))
                        .monospace()
                } else {
                    RichText::new(&ascii).color(color).monospace()
                };

                let response = ui.label(text);

                // Track hover and show tooltip
                if response.hovered() {
                    *new_hovered_range = Some(seg.range.clone());
                    if !seg.label.is_empty() && !seg.value.is_empty() {
                        response.on_hover_text(format!("{}: {}", seg.label, seg.value));
                    } else if !seg.label.is_empty() {
                        response.on_hover_text(seg.label);
                    }
                }

                pos = end;
            }
        }

        // Handle remaining bytes
        while pos < data.len() {
            let ch = if data[pos].is_ascii_graphic() || data[pos] == b' ' {
                data[pos] as char
            } else {
                '.'
            };
            ui.label(RichText::new(ch).color(Color32::DARK_GRAY).monospace());
            pos += 1;
        }

        ui.spacing_mut().item_spacing.x = prev_spacing;
    }

    /// Draw hex bytes with colors based on frame segments
    fn draw_colored_hex(
        &self,
        ui: &mut Ui,
        data: &[u8],
        segments: &[FrameSegment],
        hovered_range: Option<&Range<usize>>,
        new_hovered_range: &mut Option<Range<usize>>,
    ) {
        // Sort segments by start position
        let mut sorted_segments: Vec<_> = segments.iter().collect();
        sorted_segments.sort_by_key(|s| s.range.start);

        // Remove item spacing for tight hex display
        let prev_spacing = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 0.0;

        let mut pos = 0;
        for seg in &sorted_segments {
            // Handle any gap before this segment (shouldn't happen normally)
            while pos < seg.range.start && pos < data.len() {
                let hex = format!("{:02X} ", data[pos]);
                ui.label(RichText::new(hex).color(Color32::WHITE).monospace());
                pos += 1;
            }

            // Render this segment's bytes together
            if seg.range.start < data.len() {
                let end = seg.range.end.min(data.len());
                let hex: String = data[seg.range.start..end]
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(" ");

                // Add trailing space unless this is the last segment
                let hex_display = if end < data.len() {
                    format!("{} ", hex)
                } else {
                    hex
                };

                // Check if this segment is highlighted
                let is_highlighted = hovered_range
                    .map(|hr| hr.start == seg.range.start && hr.end == seg.range.end)
                    .unwrap_or(false);

                let color = if is_highlighted {
                    Color32::WHITE
                } else {
                    segment_color(seg.segment_type)
                };

                let text = if is_highlighted {
                    RichText::new(&hex_display)
                        .color(color)
                        .background_color(Color32::from_rgb(60, 60, 80))
                        .monospace()
                } else {
                    RichText::new(&hex_display).color(color).monospace()
                };

                let response = ui.label(text);

                // Track hover and show tooltip
                if response.hovered() {
                    *new_hovered_range = Some(seg.range.clone());
                    if !seg.label.is_empty() && !seg.value.is_empty() {
                        response.on_hover_text(format!("{}: {}", seg.label, seg.value));
                    } else if !seg.label.is_empty() {
                        response.on_hover_text(seg.label);
                    }
                }

                pos = end;
            }
        }

        // Handle any remaining bytes after all segments
        while pos < data.len() {
            let hex = if pos < data.len() - 1 {
                format!("{:02X} ", data[pos])
            } else {
                format!("{:02X}", data[pos])
            };
            ui.label(RichText::new(hex).color(Color32::WHITE).monospace());
            pos += 1;
        }

        // Restore spacing
        ui.spacing_mut().item_spacing.x = prev_spacing;
    }
}
