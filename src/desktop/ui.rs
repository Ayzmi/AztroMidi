use super::*;
use crate::appearance::*;
use crate::settings::{DropAction, Humanize, Layout, OutOfRange, Sustain};
use egui::{
    Align, Align2, Color32, FontData, FontFamily, FontId, Frame, Margin, Rect, RichText, Stroke,
    StrokeKind, Ui, Vec2,
};

pub(super) fn setup(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    for (kind, font) in [
        (egui::TextStyle::Heading, FontId::proportional(22.0)),
        (egui::TextStyle::Body, FontId::proportional(15.0)),
        (egui::TextStyle::Button, FontId::proportional(14.0)),
        (egui::TextStyle::Small, FontId::proportional(12.0)),
        (egui::TextStyle::Monospace, FontId::monospace(13.0)),
    ] {
        style.text_styles.insert(kind, font);
    }
    style.visuals = egui::Visuals::dark();
    let visuals = &mut style.visuals;
    visuals.panel_fill = BASE;
    visuals.window_fill = LIBRARY;
    visuals.window_stroke = Stroke::new(1.0_f32, OUTLINE);
    visuals.window_shadow = egui::Shadow::NONE;
    visuals.popup_shadow = egui::Shadow::NONE;
    visuals.extreme_bg_color = SURFACE;
    visuals.faint_bg_color = NAVIGATION;
    visuals.selection.bg_fill = SELECTION;
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.52 };
    visuals.slider_trailing_fill = true;
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.bg_fill = CONTROL;
        widget.weak_bg_fill = CONTROL;
        widget.bg_stroke = Stroke::new(1.0_f32, OUTLINE);
        widget.fg_stroke = Stroke::new(1.0_f32, TEXT);
        widget.corner_radius = 5.into();
        widget.expansion = 0.0;
    }
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, SEPARATOR);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.inactive.bg_fill = OUTLINE;
    visuals.widgets.hovered.weak_bg_fill = HOVER;
    visuals.widgets.hovered.bg_fill = HOVER;
    visuals.widgets.active.bg_fill = SELECTION;
    visuals.widgets.active.weak_bg_fill = SELECTION;
    visuals.widgets.active.bg_stroke.color = ACCENT;
    visuals.widgets.open.bg_stroke.color = ACCENT;
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(11.0, 6.0);
    style.spacing.interact_size = egui::vec2(36.0, 30.0);
    style.spacing.slider_width = 160.0;
    style.spacing.slider_rail_height = 4.0;
    ctx.set_style(style);

    if let Some(windows) = std::env::var_os("WINDIR") {
        let fonts = PathBuf::from(windows).join("Fonts");
        if let Ok(data) = std::fs::read(fonts.join("SegUIVar.ttf"))
            .or_else(|_| std::fs::read(fonts.join("segoeui.ttf")))
        {
            let mut definitions = egui::FontDefinitions::default();
            definitions
                .font_data
                .insert("Segoe UI".into(), Arc::new(FontData::from_owned(data)));
            definitions
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "Segoe UI".into());
            ctx.set_fonts(definitions);
        }
    }
}

fn panel(color: Color32, margin: i8) -> Frame {
    Frame::new().fill(color).inner_margin(Margin::same(margin))
}

fn card(ui: &mut Ui, contents: impl FnOnce(&mut Ui)) {
    panel(SURFACE, 15)
        .corner_radius(6)
        .stroke(Stroke::new(1.0_f32, SEPARATOR))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            contents(ui);
        });
}

fn primary(ui: &mut Ui, title: &str) -> egui::Response {
    let fill = if ui.is_enabled() { BRAND } else { CONTROL };
    ui.add(
        egui::Button::new(
            RichText::new(title)
                .color(if ui.is_enabled() { BASE } else { MUTED })
                .strong(),
        )
        .fill(fill)
        .min_size(egui::vec2(84.0, 34.0)),
    )
}

const OUTPUTS: [(Output, &str); 3] = [
    (Output::Keyboard, "Roblox"),
    (Output::Preview, "Preview on this PC"),
    (Output::Voice, "Voice Chat Piano"),
];

fn choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    choices: &[(T, &str)],
    width: f32,
) -> egui::Response {
    let selected = choices
        .iter()
        .find(|(item, _)| item == value)
        .map_or("", |(_, name)| *name);
    egui::ComboBox::from_id_salt(label)
        .width(width)
        .truncate()
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for (item, title) in choices {
                ui.selectable_value(value, *item, *title);
            }
        })
        .response
}

fn quick_choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    title: &str,
    value: &mut T,
    values: &[(T, &str)],
    width: f32,
) {
    ui.horizontal(|ui| {
        let label = ui.label(RichText::new(title).color(MUTED));
        choice(ui, title, value, values, width).labelled_by(label.id);
    });
}

fn setting_row(ui: &mut Ui, title: &str, control: impl FnOnce(&mut Ui) -> egui::Response) {
    ui.push_id(title, |ui| {
        let width = ui.available_width();
        let control_width = (width * 0.52).min(264.0);
        let label_width = (width - control_width - ui.spacing().item_spacing.x).max(0.0);
        ui.horizontal(|ui| {
            let label = ui.allocate_ui_with_layout(
                egui::vec2(label_width, 30.0),
                egui::Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_width(label_width);
                    ui.add(egui::Label::new(title).truncate())
                        .on_hover_text(title)
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(control_width, 30.0),
                egui::Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_width(control_width);
                    ui.spacing_mut().slider_width = (control_width - 88.0).max(32.0);
                    control(ui).labelled_by(label.inner.id);
                },
            );
        });
    });
}

fn setting_choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    choices: &[(T, &str)],
) -> egui::Response {
    let width = ui.available_width();
    choice(ui, label, value, choices, width)
}

fn setting_switch(ui: &mut Ui, title: &str, value: &mut bool) {
    setting_row(ui, title, |ui| {
        let text = if *value { "On" } else { "Off" };
        ui.checkbox(value, text)
    });
}

fn add_slider(ui: &mut Ui, slider: egui::Slider<'_>) -> egui::Response {
    ui.scope(|ui| {
        ui.visuals_mut().selection.bg_fill = BRAND;
        ui.add(slider)
    })
    .inner
}

fn settings_card(ui: &mut Ui, title: &str, contents: impl FnOnce(&mut Ui)) {
    card(ui, |ui| {
        ui.label(RichText::new(title).size(16.0).strong());
        ui.separator();
        ui.spacing_mut().item_spacing.y = 4.0;
        contents(ui);
    });
}

impl Desktop {
    pub(super) fn render(&mut self, ctx: &egui::Context, snapshot: Snapshot) {
        let previous_mode = self.settings.output;
        let previous_output = output_settings(&self.settings);
        let previous_schedule = schedule_settings(&self.settings);
        let previous_hotkeys = self.settings.hotkeys.clone();
        let previous_top = self.settings.always_on_top;
        egui::TopBottomPanel::bottom("status")
            .frame(panel(NAVIGATION, 8))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let color = if snapshot.waiting
                        || snapshot.output_error.is_some()
                        || snapshot.monitor_unavailable
                    {
                        WARNING
                    } else if snapshot.playing() || snapshot.live {
                        ACCENT
                    } else {
                        MUTED
                    };
                    ui.label(RichText::new(snapshot.status()).color(color).strong());
                    ui.separator();
                    if snapshot.mic_live {
                        ui.label(RichText::new("Mic live").color(ACCENT));
                        mic_meter(ui, snapshot.mic_peak, 48.0, 6.0);
                        ui.separator();
                    }
                    let hint = if let Some(error) = snapshot.output_error {
                        error.message()
                    } else if snapshot.monitor_unavailable {
                        "Monitor device unavailable. Choose another in Audio settings."
                    } else if self.library.importing() {
                        "Importing MIDI files…"
                    } else if snapshot.waiting {
                        win::OutputError::FocusLost.message()
                    } else if let Some(error) = self.save_error.as_deref() {
                        error
                    } else {
                        &self.hint
                    };
                    ui.add(egui::Label::new(RichText::new(hint).color(MUTED)).truncate());
                });
            });
        egui::SidePanel::left("navigation")
            .resizable(false)
            .exact_width(142.0)
            .frame(panel(NAVIGATION, 12))
            .show(ctx, |ui| {
                ui.label(RichText::new(APP).size(22.0).strong());
                ui.add_space(7.0);
                for page in [Page::Player, Page::Humanize, Page::Settings] {
                    let selected = self.page == page;
                    let button =
                        egui::Button::new(RichText::new(page.title()).color(if selected {
                            ACCENT
                        } else {
                            TEXT
                        }))
                        .selected(selected)
                        .corner_radius(5)
                        .stroke(Stroke::NONE);
                    if ui.add_sized([ui.available_width(), 36.0], button).clicked() {
                        self.show_page(page);
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(Align::Min), |ui| {
                    ui.add_space(2.0);
                    if ui
                        .add_sized(
                            [ui.available_width(), 32.0],
                            egui::Button::new(if self.mini.is_some() {
                                "Close mini player"
                            } else {
                                "Mini player"
                            }),
                        )
                        .on_hover_text("Ctrl+M")
                        .clicked()
                    {
                        self.toggle_mini();
                    }
                    ui.checkbox(&mut self.settings.always_on_top, "Always on top");
                });
            });

        if self.page == Page::Player {
            self.library_panel(ctx);
        }

        egui::CentralPanel::default()
            .frame(panel(BASE, 18))
            .show(ctx, |ui| {
                if self.page == Page::Settings {
                    ui.set_width(ui.available_width().min(880.0));
                    settings_header(ui, &mut self.settings_tab);
                }
                let section = (self.page == Page::Settings).then(|| self.settings_tab.title());
                egui::ScrollArea::vertical()
                    .id_salt((self.page.title(), section))
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        Page::Player => self.player(ui, snapshot),
                        Page::Humanize => self.humanize(ui),
                        Page::Settings => self.preferences(ui, snapshot),
                    });
            });
        if output_settings(&self.settings) != previous_output {
            self.configure();
        }
        if self.settings.hotkeys != previous_hotkeys {
            self.register_hotkeys();
        }
        if self.settings.always_on_top != previous_top {
            self.update_window_level();
        }
        if previous_mode != self.settings.output
            && (previous_mode == Output::Voice || self.settings.output == Output::Voice)
        {
            self.finished = 0;
            self.rebuild(false);
            if self.song.is_some() {
                self.set_loaded_hint();
            }
        } else if schedule_settings(&self.settings) != previous_schedule {
            self.defer_rebuild();
        }
        if ctx.output(|o| o.cursor_icon) == egui::CursorIcon::PointingHand {
            ctx.set_cursor_icon(egui::CursorIcon::Default);
        }
    }

    fn library_panel(&mut self, ctx: &egui::Context) {
        let max_width = (ctx.screen_rect().width() - 620.0).clamp(228.0, 318.0);
        egui::SidePanel::left("library")
            .default_width(268.0)
            .width_range(228.0..=max_width)
            .frame(panel(LIBRARY, 12))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Library").size(19.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Refresh").clicked() {
                            self.reload_library();
                        }
                        if ui.small_button("Folder").clicked() {
                            self.choose_folder();
                        }
                    });
                });
                let folder = self.library.root.to_string_lossy();
                let folder_name = self
                    .library
                    .root
                    .file_name()
                    .map_or_else(|| folder.clone(), |name| name.to_string_lossy());
                ui.add(
                    egui::Label::new(RichText::new(folder_name.as_ref()).small().color(MUTED))
                        .truncate(),
                )
                .on_hover_text(folder.as_ref());
                ui.add_sized(
                    [ui.available_width(), 29.0],
                    egui::TextEdit::singleline(&mut self.filter)
                        .id(egui::Id::new("library-search"))
                        .hint_text("Search songs"),
                );
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.favorites_only, false, "All");
                    ui.selectable_value(&mut self.favorites_only, true, "Favorites");
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .button(RichText::new("Open MIDI").color(ACCENT))
                            .on_hover_text("Ctrl+O")
                            .clicked()
                        {
                            self.choose_files();
                        }
                    });
                });

                self.refresh_visible_songs();
                let count = self.visible_songs.len();
                ui.label(RichText::new(format!("{count} songs")).small().color(MUTED));
                ui.separator();
                if count == 0 {
                    ui.label(
                        RichText::new(if self.library.songs.is_empty() {
                            "Drop MIDI files or a folder here."
                        } else {
                            "No matching songs."
                        })
                        .color(MUTED),
                    );
                }
                let mut action = None;
                egui::ScrollArea::vertical().show_rows(ui, 51.0, count, |ui, rows| {
                    for index in rows {
                        let song = &self.library.songs[self.visible_songs[index]];
                        let selected = self
                            .selected
                            .as_deref()
                            .is_some_and(|path| library::same(&song.path, path));
                        let favorite = library::favorite(&self.settings, &song.path);
                        ui.horizontal(|ui| {
                            let available = (ui.available_width() - 42.0).max(70.0);
                            let current = self.is_current(&song.path);
                            let title = RichText::new(&song.title).color(if current {
                                ACCENT
                            } else {
                                TEXT
                            });
                            let size = egui::vec2(available, 29.0);
                            let response = ui
                                .allocate_ui_with_layout(
                                    size,
                                    egui::Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.add(
                                            egui::Button::new(title)
                                                .min_size(size)
                                                .selected(selected)
                                                .corner_radius(5)
                                                .stroke(Stroke::NONE)
                                                .truncate(),
                                        )
                                    },
                                )
                                .inner
                                .on_hover_text(song.path.display().to_string());
                            if response.double_clicked() {
                                action = Some((ListAction::Play, song.path.clone()));
                            } else if response.clicked() {
                                action = Some((ListAction::Select, song.path.clone()));
                            }
                            response.context_menu(|ui| {
                                for (title, what) in [
                                    ("Play", ListAction::Play),
                                    (
                                        if favorite {
                                            "Remove favorite"
                                        } else {
                                            "Add favorite"
                                        },
                                        ListAction::Favorite,
                                    ),
                                    ("Show in folder", ListAction::Reveal),
                                    ("Copy path", ListAction::Copy),
                                    ("Remove from list", ListAction::Remove),
                                ] {
                                    if ui.button(title).clicked() {
                                        action = Some((what, song.path.clone()));
                                        ui.close_menu();
                                    }
                                }
                            });
                            if ui
                                .add_sized(
                                    [34.0, 29.0],
                                    egui::Button::new(
                                        RichText::new(if favorite { "★" } else { "☆" })
                                            .color(if favorite { ACCENT } else { MUTED }),
                                    ),
                                )
                                .on_hover_text(if favorite {
                                    "Remove favorite"
                                } else {
                                    "Add favorite"
                                })
                                .clicked()
                            {
                                action = Some((ListAction::Favorite, song.path.clone()));
                            }
                        });
                        let (length, color) = match song.duration {
                            library::SongDuration::Known(seconds) => (format_time(seconds), MUTED),
                            library::SongDuration::Pending => ("…".into(), MUTED),
                            library::SongDuration::Unreadable => {
                                ("Unreadable MIDI".into(), WARNING)
                            }
                        };
                        let subtitle = if song.folder.is_empty() {
                            length
                        } else {
                            format!("{length}  ·  {}", song.folder)
                        };
                        ui.add(
                            egui::Label::new(RichText::new(subtitle).small().color(color))
                                .truncate(),
                        );
                    }
                });
                if let Some((action, path)) = action {
                    match action {
                        ListAction::Select => self.select(path),
                        ListAction::Play => {
                            self.selected = Some(path);
                            self.play_selected();
                        }
                        ListAction::Favorite => self.favorite(&path),
                        ListAction::Reveal => self.open_folder(&path),
                        ListAction::Copy => self.ctx.copy_text(path.to_string_lossy().into_owned()),
                        ListAction::Remove => self.remove(&path),
                    }
                }
            });
    }

    fn player(&mut self, ui: &mut Ui, snapshot: Snapshot) {
        card(ui, |ui| {
            let compact = ui.available_width() < 450.0;
            if let Some(song) = &self.song {
                ui.add(egui::Label::new(RichText::new(&song.title).size(22.0).strong()).truncate())
                    .on_hover_text(&song.title);
                let used = song
                    .midi
                    .tracks
                    .iter()
                    .filter(|track| track.note_count > 0)
                    .count();
                ui.label(
                    RichText::new(format!(
                        "{} notes  ·  {used} tracks  ·  {} BPM",
                        song.midi.notes.len(),
                        song.midi.bpm,
                    ))
                    .color(MUTED),
                );
            } else {
                ui.label(RichText::new("No song loaded").size(21.0).strong());
                if ui.button("Open MIDI…").on_hover_text("Ctrl+O").clicked() {
                    self.choose_files();
                }
            }
            if snapshot.state == State::Countdown {
                ui.label(
                    RichText::new(format!("Starting in {:.0}…", snapshot.countdown.ceil()))
                        .color(ACCENT),
                );
            }
            let duration = snapshot.length;
            let mut position = self.seek_preview.unwrap_or(snapshot.position);
            let scrub = ui
                .scope(|ui| {
                    ui.spacing_mut().slider_width = (ui.available_width() - 4.0).max(1.0);
                    ui.spacing_mut().interact_size.y = 20.0;
                    ui.visuals_mut().selection.bg_fill = BRAND;
                    ui.add_enabled(
                        duration > 0.0,
                        egui::Slider::new(&mut position, 0.0..=duration.max(0.01))
                            .show_value(false)
                            .trailing_fill(true),
                    )
                    .on_hover_text("Playback position")
                })
                .inner;
            if scrub.dragged() {
                self.seek_preview = Some(position);
            }
            if scrub.drag_stopped() || (scrub.changed() && !scrub.dragged()) {
                self.engine.send(Command::Seek(position));
                self.seek_preview = None;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(format_time(position)).small().monospace());
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format_time(duration))
                            .small()
                            .monospace()
                            .color(MUTED),
                    );
                });
            });

            ui.separator();
            ui.horizontal_wrapped(|ui| {
                let has_events = !self.events.is_empty();
                if ui
                    .add_enabled_ui(has_events || snapshot.playing(), |ui| {
                        primary(ui, if snapshot.playing() { "Pause" } else { "Play" })
                    })
                    .inner
                    .clicked()
                {
                    self.toggle_play(false);
                }
                if ui
                    .add_enabled(
                        snapshot.state != State::Stopped || snapshot.live,
                        egui::Button::new("Stop"),
                    )
                    .clicked()
                {
                    self.end_live_input();
                    self.engine.send(Command::Stop);
                }
                if ui.button("Previous").clicked() {
                    self.step(-1, snapshot.playing());
                }
                if ui.button("Next").clicked() {
                    self.step(1, snapshot.playing());
                }
            });
            if compact {
                self.speed_control(ui);
                self.transpose_control(ui);
            } else {
                ui.horizontal(|ui| {
                    self.speed_control(ui);
                    self.transpose_control(ui);
                });
            }
            ui.horizontal_wrapped(|ui| {
                quick_choice(ui, "Output", &mut self.settings.output, &OUTPUTS, 145.0);
                if self.settings.output == Output::Voice {
                    if ui.small_button("Audio settings").clicked() {
                        self.settings_tab = SettingsTab::Audio;
                        self.show_page(Page::Settings);
                    }
                } else {
                    quick_choice(
                        ui,
                        "Layout",
                        &mut self.settings.layout,
                        &[(Layout::Keys61, "61 keys"), (Layout::Keys88, "88 keys")],
                        74.0,
                    );
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.settings.repeat, "Loop");
                ui.checkbox(&mut self.settings.auto_next, "Auto next");
                ui.checkbox(&mut self.settings.humanize.enabled, "Humanize");
            });
        });
        card(ui, |ui| {
            let voice = self.settings.output == Output::Voice;
            let upcoming =
                playback::upcoming(&self.events, snapshot.index, 8, self.settings.output);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Piano").strong());
                ui.separator();
                ui.label(
                    RichText::new(if voice { "Next notes" } else { "Next keys" })
                        .small()
                        .color(MUTED),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(if upcoming.is_empty() {
                            "—"
                        } else {
                            &upcoming
                        })
                        .color(ACCENT)
                        .monospace(),
                    )
                    .truncate(),
                );
            });
            piano(ui, self.settings.playback_layout(), &snapshot.held);
            if voice {
                ui.checkbox(&mut self.settings.voice.live_keyboard, "Play with keyboard");
                if self.settings.voice.live_keyboard {
                    let hint = if snapshot.playing() {
                        "Pause or stop MIDI playback to play with the keyboard."
                    } else {
                        "Use the number row and letters. Hold Shift for black keys."
                    };
                    ui.label(RichText::new(hint).small().color(MUTED));
                }
            }
        });
        if let Some(song) = &mut self.song {
            let mut changed = false;
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Tracks").strong());
                    if ui.small_button("All").clicked() {
                        song.enabled.fill(true);
                        changed = true;
                    }
                    if ui.small_button("None").clicked() {
                        song.enabled.fill(false);
                        changed = true;
                    }
                });
                for (index, track) in song.midi.tracks.iter().enumerate() {
                    if track.note_count == 0 {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        let skipped = self.settings.skip_drums && track.drums();
                        if skipped {
                            ui.label(track.name(index));
                        } else {
                            changed |= ui
                                .checkbox(&mut song.enabled[index], track.name(index))
                                .changed();
                        }
                        ui.label(
                            RichText::new(format!("{} notes", track.note_count))
                                .small()
                                .color(MUTED),
                        );
                        if track.drums() {
                            let label = if skipped { "Drums skipped" } else { "Drums" };
                            ui.label(RichText::new(label).small().color(MUTED));
                        }
                        if !skipped && ui.small_button("Solo").clicked() {
                            song.enabled.fill(false);
                            song.enabled[index] = true;
                            changed = true;
                        }
                    });
                }
            });
            if changed {
                self.rebuild(true);
            }
        }
    }

    fn speed_control(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let label = ui.label(RichText::new("Speed").color(MUTED));
            ui.spacing_mut().slider_width = 84.0;
            add_slider(
                ui,
                egui::Slider::new(&mut self.settings.speed, 0.25..=2.0)
                    .step_by(0.05)
                    .fixed_decimals(2)
                    .suffix("x"),
            )
            .labelled_by(label.id);
        });
    }

    fn transpose_control(&mut self, ui: &mut Ui) {
        let Some(song) = &mut self.song else { return };
        let voice = self.settings.output == Output::Voice;
        let current = song.transpose(self.settings.output);
        let transpose = ui
            .horizontal(|ui| {
                ui.label(RichText::new("Transpose").color(MUTED));
                let down = ui.button("−").on_hover_text("Transpose down").clicked();
                ui.monospace(format!("{current:+}"));
                let up = ui.button("+").on_hover_text("Transpose up").clicked();
                if ui
                    .small_button(if voice { "Reset" } else { "Auto" })
                    .clicked()
                {
                    if voice {
                        0
                    } else {
                        auto_transpose(song, &self.settings)
                    }
                } else {
                    (current + i32::from(up) - i32::from(down)).clamp(-48, 48)
                }
            })
            .inner;
        if transpose != current {
            if voice {
                song.voice_transpose = transpose;
            } else {
                song.transpose = transpose;
            }
            self.rebuild(true);
        }
    }

    fn humanize(&mut self, ui: &mut Ui) {
        ui.heading("Humanize");
        let h = &mut self.settings.humanize;
        settings_card(ui, "Presets", |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.selectable_label(!h.enabled, "Off").clicked() {
                    h.enabled = false;
                }
                for (label, values) in [
                    ("Subtle", (4.0, 8.0, 1.0, 6.0, 3, 15.0, 0.0)),
                    ("Natural", (8.0, 14.0, 2.5, 12.0, 6, 35.0, 0.0)),
                    ("Expressive", (14.0, 24.0, 4.5, 20.0, 10, 70.0, 0.4)),
                    ("Sloppy", (22.0, 34.0, 6.0, 28.0, 14, 90.0, 2.0)),
                ] {
                    let (jitter, roll, drift, hold, velocity, breath, mistakes) = values;
                    let preset = Humanize {
                        enabled: true,
                        timing_jitter_ms: jitter,
                        chord_roll_ms: roll,
                        tempo_drift_percent: drift,
                        hold_variance_percent: hold,
                        velocity_variance: velocity,
                        phrase_breath_ms: breath,
                        mistake_chance_percent: mistakes,
                    };
                    if ui.selectable_label(*h == preset, label).clicked() {
                        *h = preset;
                    }
                }
            });
            ui.checkbox(&mut h.enabled, "Enable humanize");
        });
        settings_card(ui, "Adjustments", |ui| {
            slider(
                ui,
                &mut h.timing_jitter_ms,
                0.0,
                40.0,
                1.0,
                " ms",
                "Timing looseness",
            );
            slider(
                ui,
                &mut h.chord_roll_ms,
                0.0,
                60.0,
                1.0,
                " ms",
                "Chord roll",
            );
            slider(
                ui,
                &mut h.tempo_drift_percent,
                0.0,
                10.0,
                0.5,
                "%",
                "Tempo drift",
            );
            slider(
                ui,
                &mut h.hold_variance_percent,
                0.0,
                40.0,
                1.0,
                "%",
                "Key hold variation",
            );
            slider(
                ui,
                &mut h.phrase_breath_ms,
                0.0,
                150.0,
                5.0,
                " ms",
                "Phrase breathing",
            );
            setting_row(ui, "Velocity variation", |ui| {
                add_slider(ui, egui::Slider::new(&mut h.velocity_variance, 0..=30))
            });
            slider(
                ui,
                &mut h.mistake_chance_percent,
                0.0,
                5.0,
                0.1,
                "%",
                "Wrong-note slips",
            );
        });
    }

    fn preferences(&mut self, ui: &mut Ui, snapshot: Snapshot) {
        match self.settings_tab {
            SettingsTab::Audio => self.audio_settings(ui, snapshot),
            SettingsTab::LibraryAndWindow => self.library_settings(ui),
            SettingsTab::Hotkeys => self.hotkey_settings(ui),
        }
    }

    fn audio_settings(&mut self, ui: &mut Ui, snapshot: Snapshot) {
        let s = &mut self.settings;
        audio_devices(ui, s, &mut self.voice_devices);
        if s.output == Output::Voice {
            voice_mixer(ui, &mut s.voice, snapshot);
        }
        settings_card(ui, "MIDI playback", |ui| {
            setting_row(ui, "Out-of-range notes", |ui| {
                setting_choice(
                    ui,
                    "out-of-range",
                    &mut s.out_of_range,
                    &[
                        (OutOfRange::Fold, "Fold by octave"),
                        (OutOfRange::Skip, "Skip"),
                    ],
                )
            });
            setting_switch(ui, "Skip drum tracks", &mut s.skip_drums);
            if s.output != Output::Voice {
                setting_row(ui, "Sustain pedal", |ui| {
                    setting_choice(
                        ui,
                        "sustain",
                        &mut s.sustain,
                        &[
                            (Sustain::Off, "Off"),
                            (Sustain::FollowPedal, "Follow MIDI pedal"),
                        ],
                    )
                });
                setting_switch(ui, "Velocity keys", &mut s.velocity_keys);
                setting_switch(ui, "Auto transpose on load", &mut s.auto_transpose);
                setting_switch(ui, "Switch to Roblox on Play", &mut s.auto_focus_roblox);
                setting_row(ui, "Countdown", |ui| {
                    add_slider(
                        ui,
                        egui::Slider::new(&mut s.countdown_seconds, 0..=10).suffix(" s"),
                    )
                });
                slider(
                    ui,
                    &mut s.min_hold_ms,
                    5.0,
                    120.0,
                    5.0,
                    " ms",
                    "Shortest key press",
                );
            }
        });
    }

    fn library_settings(&mut self, ui: &mut Ui) {
        let s = &mut self.settings;
        settings_card(ui, "Library", |ui| {
            setting_row(ui, "Dropped files", |ui| {
                setting_choice(
                    ui,
                    "dropped files",
                    &mut s.drop_action,
                    &[
                        (DropAction::Move, "Move into library"),
                        (DropAction::Copy, "Copy into library"),
                        (DropAction::ListOnly, "Only add to list"),
                    ],
                )
            });
        });
        let mut mini = false;
        settings_card(ui, "Window and mini player", |ui| {
            setting_switch(ui, "Always on top", &mut s.always_on_top);
            setting_switch(ui, "Stick to Roblox", &mut s.mini_snap_to_roblox);
            let mut opacity = s.mini_opacity * 100.0;
            slider(ui, &mut opacity, 40.0, 100.0, 5.0, "%", "Opacity");
            s.mini_opacity = opacity / 100.0;
            setting_row(ui, "Overlay", |ui| {
                let button = ui.button(if self.mini.is_some() {
                    "Close mini player"
                } else {
                    "Open mini player"
                });
                mini = button.clicked();
                button
            });
        });
        if mini {
            self.toggle_mini();
        }
    }

    fn hotkey_settings(&mut self, ui: &mut Ui) {
        settings_card(ui, "Global hotkeys", |ui| {
            for (binding, label) in self.settings.hotkeys.bindings_mut().into_iter().zip([
                "Play / Pause",
                "Stop",
                "Slow down",
                "Speed up",
                "Next song",
                "Toggle humanize",
                "Toggle mini player",
            ]) {
                setting_row(ui, label, |ui| {
                    egui::ComboBox::from_id_salt("hotkey")
                        .width(ui.available_width())
                        .truncate()
                        .selected_text(binding.as_str())
                        .show_ui(ui, |ui| {
                            for key in win::HOTKEYS {
                                if ui
                                    .selectable_label(binding.eq_ignore_ascii_case(key), key)
                                    .clicked()
                                {
                                    *binding = key.into();
                                }
                            }
                        })
                        .response
                });
            }
            if !self.hotkey_errors.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "Hotkeys unavailable: {}",
                        self.hotkey_errors.join(", ")
                    ))
                    .color(WARNING),
                );
            }
        });
    }
}

#[derive(Clone, Copy)]
enum ListAction {
    Select,
    Play,
    Favorite,
    Reveal,
    Copy,
    Remove,
}

fn settings_header(ui: &mut Ui, current: &mut SettingsTab) {
    ui.heading("Settings");
    ui.horizontal_wrapped(|ui| {
        for tab in [
            SettingsTab::Audio,
            SettingsTab::LibraryAndWindow,
            SettingsTab::Hotkeys,
        ] {
            let selected = *current == tab;
            let text = RichText::new(tab.title()).color(if selected { ACCENT } else { MUTED });
            let button = egui::Button::new(text)
                .selected(selected)
                .stroke(Stroke::NONE)
                .min_size(egui::vec2(80.0, 30.0));
            if ui.add(button).clicked() {
                *current = tab;
            }
        }
    });
    ui.add_space(4.0);
    ui.separator();
}

fn audio_devices(
    ui: &mut Ui,
    settings: &mut Settings,
    devices: &mut Option<Result<crate::voice::Devices, String>>,
) {
    card(ui, |ui| {
        let mut refresh = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Audio devices").size(16.0).strong());
            if settings.output == Output::Voice {
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    refresh = ui.small_button("Refresh devices").clicked();
                });
            }
        });
        ui.separator();
        ui.spacing_mut().item_spacing.y = 4.0;
        setting_row(ui, "Output", |ui| {
            setting_choice(ui, "output", &mut settings.output, &OUTPUTS)
        });
        if settings.output == Output::Voice {
            voice_devices(ui, &mut settings.voice, devices, refresh);
        }
    });
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    use crate::voice::{Devices, Endpoint};

    #[test]
    fn voice_settings_fit_with_long_device_names_and_live_input() {
        let ctx = egui::Context::default();
        setup(&ctx);
        let mut settings = Settings {
            output: Output::Voice,
            ..Settings::default()
        };
        settings.voice.microphone = "mic".into();
        settings.voice.monitor = "headphones".into();
        let mut tab = SettingsTab::Audio;
        let mut devices = Some(Ok(Devices {
            destinations: vec![
                Endpoint {
                    id: "cable".into(),
                    name: "CABLE Input (VB-Audio Virtual Cable)".into(),
                },
                Endpoint {
                    id: "headphones".into(),
                    name: "Headphones (Realtek(R) Audio Digital Playback Device)".into(),
                },
            ],
            microphones: vec![Endpoint {
                id: "mic".into(),
                name: "Microphone (6- Rexus Xora-II Microphone USB Audio Device)".into(),
            }],
        }));
        let snapshot = Snapshot {
            mic_live: true,
            mic_peak: 0.17,
            ..Snapshot::default()
        };
        let width = 820;
        let height = 640;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32, height as f32),
            )),
            ..Default::default()
        };
        let mut bounds = [0.0; 4];
        let _ = ctx.run(input, |ctx| {
            egui::SidePanel::left("navigation")
                .exact_width(142.0)
                .frame(panel(NAVIGATION, 12))
                .show(ctx, |ui| {
                    ui.label(RichText::new(APP).size(22.0).strong());
                    ui.add_space(7.0);
                    for page in [Page::Player, Page::Humanize, Page::Settings] {
                        let button =
                            egui::Button::new(page.title()).selected(page == Page::Settings);
                        let _ = ui.add_sized([ui.available_width(), 36.0], button);
                    }
                });
            egui::CentralPanel::default()
                .frame(panel(BASE, 18))
                .show(ctx, |ui| {
                    bounds[0] = ui.max_rect().right();
                    settings_header(ui, &mut tab);
                    bounds[1] = ui.min_rect().right();
                    audio_devices(ui, &mut settings, &mut devices);
                    bounds[2] = ui.min_rect().right();
                    voice_mixer(ui, &mut settings.voice, snapshot);
                    bounds[3] = ui.min_rect().right();
                });
        });
        assert!(
            bounds[3] <= bounds[0] + 0.5,
            "settings exceed their column: {bounds:?}"
        );
    }
}

fn voice_devices(
    ui: &mut Ui,
    settings: &mut crate::settings::Voice,
    devices: &mut Option<Result<crate::voice::Devices, String>>,
    refresh: bool,
) {
    if refresh || devices.is_none() {
        *devices = Some(crate::voice::Devices::scan().map_err(|error| error.to_string()));
    }
    match devices.as_ref() {
        Some(Ok(devices)) => {
            let auto = if devices.cable().is_some() {
                "Automatic (VB-CABLE)"
            } else {
                "Automatic (VB-CABLE missing)"
            };
            endpoint_choice(
                ui,
                "Send audio to",
                &mut settings.destination,
                auto,
                devices.destinations.iter(),
            );
            endpoint_choice(
                ui,
                "Microphone",
                &mut settings.microphone,
                "Off (piano only)",
                devices.microphones.iter(),
            );
            let destination = &settings.destination;
            if settings.monitor == *destination {
                settings.monitor.clear();
            }
            let monitors = devices
                .destinations
                .iter()
                .filter(|device| !device.is_virtual_cable() && device.id != *destination);
            endpoint_choice(
                ui,
                "Monitor piano on",
                &mut settings.monitor,
                "Off",
                monitors,
            );
            if settings.destination.is_empty() && devices.cable().is_none() {
                ui.hyperlink_to("Get VB-CABLE for voice chat", "https://vb-audio.com/Cable/");
            }
        }
        Some(Err(error)) => {
            ui.label(RichText::new("Windows audio devices could not be listed.").color(WARNING))
                .on_hover_text(error);
        }
        None => {}
    }
    ui.add_space(3.0);
    ui.label(
        RichText::new("In Roblox, choose CABLE Output as the microphone for VB-CABLE.")
            .small()
            .color(MUTED),
    );
    if settings.microphone.is_empty() {
        ui.label(
            RichText::new("Select your physical microphone above to speak or sing with piano.")
                .small()
                .color(ACCENT),
        );
    }
}

fn voice_mixer(ui: &mut Ui, settings: &mut crate::settings::Voice, snapshot: Snapshot) {
    settings_card(ui, "Mixer", |ui| {
        setting_row(ui, "Piano level", |ui| {
            add_slider(
                ui,
                egui::Slider::new(&mut settings.piano_level, 0.0..=100.0)
                    .step_by(1.0)
                    .max_decimals(0)
                    .suffix("%"),
            )
        });
        if !settings.microphone.is_empty() {
            setting_row(ui, "Keep mic live", |ui| {
                let state = if settings.keep_mic_live { "On" } else { "Off" };
                ui.checkbox(&mut settings.keep_mic_live, state)
                    .on_hover_text(
                        "Send microphone audio between songs with Voice Chat Piano selected.",
                    )
            });
            setting_row(ui, "Microphone level", |ui| {
                add_slider(
                    ui,
                    egui::Slider::new(&mut settings.mic_level, 0.0..=100.0)
                        .step_by(1.0)
                        .max_decimals(0)
                        .suffix("%"),
                )
            });
            setting_row(ui, "Mic signal", |ui| {
                if snapshot.mic_live {
                    let width = ui.available_width();
                    mic_meter(ui, snapshot.mic_peak, width, 8.0)
                } else {
                    ui.label(
                        RichText::new("Play a song or turn on Keep mic live")
                            .small()
                            .color(MUTED),
                    )
                }
            });
        }
        if !settings.monitor.is_empty() {
            setting_row(ui, "Monitor level", |ui| {
                add_slider(
                    ui,
                    egui::Slider::new(&mut settings.monitor_level, 0.0..=100.0)
                        .step_by(1.0)
                        .max_decimals(0)
                        .suffix("%"),
                )
            });
        }
        setting_row(ui, "Room reverb", |ui| {
            add_slider(
                ui,
                egui::Slider::new(&mut settings.room, 0.0..=60.0)
                    .step_by(1.0)
                    .max_decimals(0)
                    .suffix("%"),
            )
        });
        ui.horizontal_wrapped(|ui| {
            ui.hyperlink_to(
                RichText::new("Salamander Grand Piano V3").small(),
                "https://github.com/sfzinstruments/SalamanderGrandPiano",
            );
            ui.label(
                RichText::new("by Alexander Holm (adapted)")
                    .small()
                    .color(MUTED),
            );
            ui.hyperlink_to(
                RichText::new("CC BY 3.0").small(),
                "https://creativecommons.org/licenses/by/3.0/",
            );
        });
    });
}

fn mic_meter(ui: &mut Ui, peak: f32, width: f32, height: f32) -> egui::Response {
    ui.scope(|ui| {
        ui.visuals_mut().extreme_bg_color = CONTROL;
        ui.add(
            egui::ProgressBar::new(peak.sqrt())
                .fill(ACCENT)
                .desired_width(width)
                .desired_height(height),
        )
        .on_hover_text("Live microphone input level")
    })
    .inner
}

fn endpoint_choice<'a>(
    ui: &mut Ui,
    label: &str,
    selected: &mut String,
    default: &str,
    devices: impl Iterator<Item = &'a crate::voice::Endpoint> + Clone,
) {
    setting_row(ui, label, |ui| {
        let text = if selected.is_empty() {
            default
        } else {
            devices
                .clone()
                .find(|device| device.id == *selected)
                .map_or("Device unavailable", |device| device.name.as_str())
        };
        egui::ComboBox::from_id_salt(label)
            .width(ui.available_width())
            .truncate()
            .selected_text(text)
            .show_ui(ui, |ui| {
                if ui.selectable_label(selected.is_empty(), default).clicked() {
                    selected.clear();
                }
                for device in devices {
                    if ui
                        .selectable_label(device.id == *selected, &device.name)
                        .clicked()
                    {
                        selected.clone_from(&device.id);
                    }
                }
            })
            .response
            .on_hover_text(text)
    });
}

fn slider(ui: &mut Ui, value: &mut f64, min: f64, max: f64, step: f64, unit: &str, title: &str) {
    setting_row(ui, title, |ui| {
        add_slider(
            ui,
            egui::Slider::new(value, min..=max)
                .step_by(step)
                .suffix(unit),
        )
    });
}

fn output_settings(settings: &Settings) -> (Output, f64, bool, crate::settings::Voice) {
    (
        settings.output,
        settings.speed,
        settings.velocity_keys,
        settings.voice.clone(),
    )
}

fn schedule_settings(settings: &Settings) -> (Layout, Humanize, OutOfRange, Sustain, bool, f64) {
    (
        settings.layout,
        settings.humanize.clone(),
        settings.out_of_range,
        settings.sustain,
        settings.skip_drums,
        settings.min_hold_ms,
    )
}

fn piano(ui: &mut Ui, layout: Layout, held: &[bool; 128]) {
    let (first, last) = keys::range(layout);
    let whites = (first..=last).filter(|&n| !keys::is_black(n)).count();
    let (outer, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 92.0), egui::Sense::hover());
    if !ui.is_rect_visible(outer) {
        return;
    }
    let painter = ui.painter_at(outer);
    let width = outer.width() / whites as f32;
    let mut column = 0;
    for pitch in first..=last {
        if keys::is_black(pitch) {
            continue;
        }
        let left = outer.left() + column as f32 * width;
        let rect = Rect::from_min_size(
            Pos2::new(left, outer.top()),
            egui::vec2(width, outer.height()),
        );
        painter.rect(
            rect,
            1,
            if held[pitch as usize] {
                BRAND
            } else {
                Color32::from_rgb(224, 228, 234)
            },
            Stroke::new(1.0_f32, NAVIGATION),
            StrokeKind::Inside,
        );
        if width >= 8.0
            && let Some(key) = keys::lookup(pitch as u8)
        {
            painter.text(
                rect.center_bottom() - egui::vec2(0.0, 4.0),
                Align2::CENTER_BOTTOM,
                key.label as char,
                FontId::monospace((width * 0.84).clamp(7.0, 10.0)),
                BASE,
            );
        }
        column += 1;
    }
    let mut column = 0;
    for pitch in first..=last {
        if !keys::is_black(pitch) {
            column += 1;
            continue;
        }
        let x = outer.left() + column as f32 * width;
        let rect = Rect::from_min_size(
            Pos2::new(x - width * 0.33, outer.top()),
            egui::vec2(width * 0.66, 54.0),
        );
        painter.rect_filled(rect, 2, if held[pitch as usize] { BRAND } else { BASE });
        if width >= 12.0
            && let Some(key) = keys::lookup(pitch as u8)
        {
            painter.text(
                rect.center_bottom() - egui::vec2(0.0, 3.0),
                Align2::CENTER_BOTTOM,
                key.label as char,
                FontId::monospace(9.0),
                if held[pitch as usize] { BASE } else { TEXT },
            );
        }
    }
}
