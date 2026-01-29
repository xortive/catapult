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

/// Minimum bytes per line (don't wrap smaller than this)
const MIN_BYTES_PER_LINE: usize = 4;

/// A visual row in the traffic monitor, mapping to a specific line of an entry
struct VisualRow {
    entry_idx: usize,
    line_offset: usize, // 0 = first line with metadata, 1+ = continuation lines
}

/// Calculate the number of visual lines needed to display an entry
fn lines_for_entry(entry: &TrafficEntry, show_hex: bool, bytes_per_line: usize) -> usize {
    match entry {
        TrafficEntry::Data { data, .. } if show_hex && !data.is_empty() => {
            data.len().div_ceil(bytes_per_line)
        }
        _ => 1,
    }
}

/// Calculate how many bytes can fit per line given available width
/// Returns (bytes_per_line, char_width) - char_width is for continuation line alignment
fn calculate_bytes_per_line(ui: &Ui, available_width: f32) -> usize {
    let char_width =
        ui.fonts_mut(|f| f.glyph_width(&egui::TextStyle::Monospace.resolve(ui.style()), ' '));

    // Estimate metadata width: timestamp (12 chars) + direction badge (~10) + protocol badge (~8) + spacing
    // This is approximate - first line will have variable metadata, continuation lines just have offset
    let metadata_width = 40.0 * char_width;

    // Each byte needs: 2 hex chars + 1 space + 1 ASCII char = 4 chars
    // Plus spacing between hex and ASCII sections (8.0 * 2 = 16 pixels, roughly 2 chars)
    let chars_per_byte = 4.0;
    let section_spacing = 2.0; // Extra chars for spacing between sections

    let remaining_width = (available_width - metadata_width).max(0.0);
    let bytes_that_fit = ((remaining_width / char_width) - section_spacing) / chars_per_byte;

    // Clamp to reasonable range
    (bytes_that_fit as usize).max(MIN_BYTES_PER_LINE)
}

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
        // Toolbar - use horizontal_wrapped to allow narrower panels
        ui.horizontal_wrapped(|ui| {
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
                    ui.close();
                }
                if ui.button("Save to File...").clicked() {
                    export_action = Some(match self.save_filtered_log_with_dialog() {
                        Ok(Some(path)) => ExportAction::SavedToFile(path),
                        Ok(None) => ExportAction::Cancelled,
                        Err(e) => ExportAction::Error(e),
                    });
                    ui.close();
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
                        ui.close();
                    }
                    if ui
                        .selectable_label(self.diagnostic_level == Some(Level::ERROR), "Error")
                        .clicked()
                    {
                        self.diagnostic_level = Some(Level::ERROR);
                        ui.close();
                    }
                    if ui
                        .selectable_label(self.diagnostic_level == Some(Level::WARN), "Warning")
                        .clicked()
                    {
                        self.diagnostic_level = Some(Level::WARN);
                        ui.close();
                    }
                    if ui
                        .selectable_label(self.diagnostic_level == Some(Level::INFO), "Info")
                        .clicked()
                    {
                        self.diagnostic_level = Some(Level::INFO);
                        ui.close();
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
                        ui.close();
                    }
                },
            );
        });

        ui.separator();

        // Calculate bytes per line based on available width
        let available_width = ui.available_width();
        let bytes_per_line = calculate_bytes_per_line(ui, available_width);

        // Traffic list - build visual rows for proper virtual scrolling with line wrapping
        let visual_rows: Vec<VisualRow> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| self.entry_passes_filter(entry))
            .flat_map(|(entry_idx, entry)| {
                let num_lines = lines_for_entry(entry, show_hex, bytes_per_line);
                (0..num_lines).map(move |line_offset| VisualRow {
                    entry_idx,
                    line_offset,
                })
            })
            .collect();

        let text_style = egui::TextStyle::Monospace;
        let row_height = ui.text_style_height(&text_style);

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .stick_to_bottom(self.auto_scroll)
            .show_rows(ui, row_height, visual_rows.len(), |ui, row_range| {
                for i in row_range {
                    if let Some(visual_row) = visual_rows.get(i) {
                        if let Some(entry) = self.entries.get(visual_row.entry_idx) {
                            self.draw_entry(
                                ui,
                                entry,
                                visual_row.entry_idx,
                                visual_row.line_offset,
                                bytes_per_line,
                                show_hex,
                                show_decoded,
                            );
                        }
                    }
                }
                // Bottom margin to prevent scroll jitter during autoscroll
                ui.add_space(row_height);
            });

        export_action
    }

    /// Draw a single line of a traffic entry
    #[allow(clippy::too_many_arguments)]
    fn draw_entry(
        &self,
        ui: &mut Ui,
        entry: &TrafficEntry,
        entry_idx: usize,
        line_offset: usize,
        bytes_per_line: usize,
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
                    line_offset,
                    bytes_per_line,
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
                // Diagnostics are always single line, line_offset is ignored
                if line_offset == 0 {
                    self.draw_diagnostic_entry(ui, timestamp, source, severity, message);
                }
            }
        }
    }

    /// Draw a single line of a data traffic entry
    #[allow(clippy::too_many_arguments)]
    fn draw_data_entry(
        &self,
        ui: &mut Ui,
        entry_idx: usize,
        line_offset: usize,
        bytes_per_line: usize,
        timestamp: &SystemTime,
        source: &TrafficSource,
        data: &[u8],
        decoded: Option<&AnnotatedFrame>,
        show_hex: bool,
        show_decoded: bool,
    ) {
        // Calculate byte range for this line
        let start_byte = line_offset * bytes_per_line;
        let end_byte = (start_byte + bytes_per_line).min(data.len());

        // Safety check: don't render if start is past data length
        if start_byte >= data.len() && !data.is_empty() {
            return;
        }

        ui.horizontal(|ui| {
            // Create a unique ID for this entry's hover state (shared across all lines)
            let hover_id = Id::new("traffic_hover").with(entry_idx);

            // Get the currently hovered byte range from previous frame
            let hovered_range: Option<Range<usize>> = ui.memory(|mem| mem.data.get_temp(hover_id));

            // Track new hover state for this frame
            let mut new_hovered_range: Option<Range<usize>> = None;

            if line_offset == 0 {
                // First line: show metadata (timestamp, direction, protocol, decoded summary)

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

                // Direction indicator with source info
                match source {
                    TrafficSource::RealRadio { port, .. } => {
                        let label = if port.is_empty() {
                            "[Radio→]".to_string()
                        } else {
                            format!("[{}→]", port)
                        };
                        ui.label(RichText::new(label).color(Color32::LIGHT_BLUE).monospace());
                    }
                    TrafficSource::ToRealRadio { port, .. } => {
                        let label = if port.is_empty() {
                            "[→Radio]".to_string()
                        } else {
                            format!("[→{}]", port)
                        };
                        ui.label(
                            RichText::new(label)
                                .color(Color32::from_rgb(180, 100, 255)) // Purple for outgoing to radio
                                .monospace(),
                        );
                    }
                    TrafficSource::RealAmplifier { port } => {
                        let label = if port.is_empty() {
                            "[→Amp]".to_string()
                        } else {
                            format!("[→{}]", port)
                        };
                        ui.label(RichText::new(label).color(Color32::LIGHT_GREEN).monospace());
                    }
                    TrafficSource::FromRealAmplifier { port } => {
                        let label = if port.is_empty() {
                            "[Amp→]".to_string()
                        } else {
                            format!("[{}→]", port)
                        };
                        ui.label(RichText::new(label).color(Color32::LIGHT_GREEN).monospace());
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
            } else {
                // Continuation line: show offset prefix aligned with hex position
                // Format: "         00000010: " (spaces to align with metadata, then offset)
                let offset_prefix = format!("{:08X}:", start_byte);
                ui.label(
                    RichText::new(offset_prefix)
                        .color(Color32::DARK_GRAY)
                        .monospace(),
                );
            }

            // ASCII representation with highlighting (for this line's bytes only)
            if show_hex && !data.is_empty() {
                ui.add_space(8.0);
                let line_data = &data[start_byte..end_byte];
                if let Some(decoded) = decoded {
                    self.draw_ascii_with_segments(
                        ui,
                        line_data,
                        start_byte,
                        &decoded.segments,
                        hovered_range.as_ref(),
                        &mut new_hovered_range,
                    );
                } else {
                    let ascii: String = line_data
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
            if show_hex && !data.is_empty() {
                ui.add_space(8.0);
                let line_data = &data[start_byte..end_byte];
                if let Some(decoded) = decoded {
                    self.draw_colored_hex(
                        ui,
                        line_data,
                        start_byte,
                        &decoded.segments,
                        hovered_range.as_ref(),
                        &mut new_hovered_range,
                    );
                } else {
                    let hex: String = line_data
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
    /// `data` contains only the bytes for this line
    /// `byte_offset` is the global byte offset where this line's data starts
    fn draw_ascii_with_segments(
        &self,
        ui: &mut Ui,
        data: &[u8],
        byte_offset: usize,
        segments: &[FrameSegment],
        hovered_range: Option<&Range<usize>>,
        new_hovered_range: &mut Option<Range<usize>>,
    ) {
        let line_end = byte_offset + data.len();

        // Filter and translate segments to line-local coordinates
        let mut sorted_segments: Vec<_> = segments
            .iter()
            .filter(|s| s.range.start < line_end && s.range.end > byte_offset)
            .collect();
        sorted_segments.sort_by_key(|s| s.range.start);

        let prev_spacing = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 0.0;

        let mut pos = 0usize; // Local position within this line's data
        for seg in &sorted_segments {
            // Calculate local segment bounds
            let seg_local_start = seg.range.start.saturating_sub(byte_offset);
            let seg_local_end = (seg.range.end - byte_offset).min(data.len());

            // Handle gap before segment
            while pos < seg_local_start && pos < data.len() {
                let ch = if data[pos].is_ascii_graphic() || data[pos] == b' ' {
                    data[pos] as char
                } else {
                    '.'
                };
                ui.label(RichText::new(ch).color(Color32::DARK_GRAY).monospace());
                pos += 1;
            }

            // Render segment's ASCII (only the portion visible on this line)
            if seg_local_start < data.len() {
                let ascii: String = data[seg_local_start..seg_local_end]
                    .iter()
                    .map(|&b| {
                        if b.is_ascii_graphic() || b == b' ' {
                            b as char
                        } else {
                            '.'
                        }
                    })
                    .collect();

                // Check if this segment is highlighted (using original global range)
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

                // Track hover and show tooltip (use original global range for hover state)
                if response.hovered() {
                    *new_hovered_range = Some(seg.range.clone());
                    if !seg.label.is_empty() && !seg.value.is_empty() {
                        response.on_hover_text(format!("{}: {}", seg.label, seg.value));
                    } else if !seg.label.is_empty() {
                        response.on_hover_text(seg.label);
                    }
                }

                pos = seg_local_end;
            }
        }

        // Handle remaining bytes after all segments
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
    /// `data` contains only the bytes for this line
    /// `byte_offset` is the global byte offset where this line's data starts
    fn draw_colored_hex(
        &self,
        ui: &mut Ui,
        data: &[u8],
        byte_offset: usize,
        segments: &[FrameSegment],
        hovered_range: Option<&Range<usize>>,
        new_hovered_range: &mut Option<Range<usize>>,
    ) {
        let line_end = byte_offset + data.len();

        // Filter and sort segments that overlap with this line
        let mut sorted_segments: Vec<_> = segments
            .iter()
            .filter(|s| s.range.start < line_end && s.range.end > byte_offset)
            .collect();
        sorted_segments.sort_by_key(|s| s.range.start);

        // Remove item spacing for tight hex display
        let prev_spacing = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 0.0;

        let mut pos = 0usize; // Local position within this line's data
        for seg in &sorted_segments {
            // Calculate local segment bounds
            let seg_local_start = seg.range.start.saturating_sub(byte_offset);
            let seg_local_end = (seg.range.end - byte_offset).min(data.len());

            // Handle any gap before this segment
            while pos < seg_local_start && pos < data.len() {
                let hex = format!("{:02X} ", data[pos]);
                ui.label(RichText::new(hex).color(Color32::WHITE).monospace());
                pos += 1;
            }

            // Render this segment's bytes (only the portion visible on this line)
            if seg_local_start < data.len() {
                let hex: String = data[seg_local_start..seg_local_end]
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(" ");

                // Add trailing space unless this is the end of the line
                let hex_display = if seg_local_end < data.len() {
                    format!("{} ", hex)
                } else {
                    hex
                };

                // Check if this segment is highlighted (using original global range)
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

                // Track hover and show tooltip (use original global range for hover state)
                if response.hovered() {
                    *new_hovered_range = Some(seg.range.clone());
                    if !seg.label.is_empty() && !seg.value.is_empty() {
                        response.on_hover_text(format!("{}: {}", seg.label, seg.value));
                    } else if !seg.label.is_empty() {
                        response.on_hover_text(seg.label);
                    }
                }

                pos = seg_local_end;
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
