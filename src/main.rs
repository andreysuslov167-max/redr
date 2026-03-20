use eframe::egui;
use tinyfiledialogs;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
use std::thread;

struct TextEditor {
    content: String,
    current_file: Option<PathBuf>,
    unsaved_changes: bool,
    zoom_level: f32,
    cursor_position: (usize, usize),
    syntax_highlighting: bool,
    terminal_output: String,
    terminal_receiver: Option<Receiver<String>>,
    is_running: bool,
}

impl Default for TextEditor {
    fn default() -> Self {
        Self {
            content: String::new(),
            current_file: None,
            unsaved_changes: false,
            zoom_level: 14.0,
            cursor_position: (0, 0),
            syntax_highlighting: true,
            terminal_output: String::new(),
            terminal_receiver: None,
            is_running: false,
        }
    }
}

impl TextEditor {
    fn run_command(&mut self, cmd: &str) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.terminal_receiver = Some(rx);
        self.terminal_output.clear();
        self.is_running = true;

        let cmd_string = cmd.to_string();

        thread::spawn(move || {
            let mut child = if cfg!(target_os = "windows") {
                Command::new("cmd")
                    .args(["/C", &cmd_string])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
            } else {
                Command::new("sh")
                    .arg("-c")
                    .arg(&cmd_string)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
            }.expect("Failed to start process");

            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();

            let tx_clone = tx.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    tx_clone.send(line + "\n").ok();
                }
            });

            let reader_err = BufReader::new(stderr);
            for line in reader_err.lines().map_while(Result::ok) {
                tx.send(format!("ERR: {}\n", line)).ok();
            }
        });
    }

    fn open_file(&mut self) {
        let path = tinyfiledialogs::open_file_dialog(
            "Открыть файл",
            "",
            Some((&["*.txt", "*.rs", "*.md", "*.toml"], "Текстовые файлы"))
        );

        if let Some(path_str) = path {
            let path = PathBuf::from(path_str);
            match fs::read_to_string(&path) {
                Ok(contents) => {
                    self.content = contents;
                    self.current_file = Some(path);
                    self.unsaved_changes = false;
                }
                Err(e) => {
                    tinyfiledialogs::message_box_ok(
                        "Ошибка",
                        &format!("Не удалось открыть файл: {}", e),
                        tinyfiledialogs::MessageBoxIcon::Error
                    );
                }
            }
        }
    }

    fn save_file(&mut self) {
        if let Some(path) = &self.current_file {
            if let Err(e) = fs::write(path, &self.content) {
                tinyfiledialogs::message_box_ok(
                    "Ошибка",
                    &format!("Не удалось сохранить файл: {}", e),
                    tinyfiledialogs::MessageBoxIcon::Error
                );
            } else {
                self.unsaved_changes = false;
            }
        } else {
            self.save_file_as();
        }
    }

    fn save_file_as(&mut self) {
        let path_str = tinyfiledialogs::save_file_dialog(
            "Сохранить файл",
            "новый_файл.txt"
        );

        if let Some(path_str) = path_str {
            let path = PathBuf::from(path_str);
            if let Err(e) = fs::write(&path, &self.content) {
                tinyfiledialogs::message_box_ok(
                    "Ошибка",
                    &format!("Не удалось сохранить файл: {}", e),
                    tinyfiledialogs::MessageBoxIcon::Error
                );
            } else {
                self.current_file = Some(path);
                self.unsaved_changes = false;
            }
        }
    }

    fn new_file(&mut self) {
        if self.unsaved_changes {
            let result = tinyfiledialogs::message_box_yes_no(
                "Несохраненные изменения",
                "Сохранить изменения перед созданием нового файла?",
                tinyfiledialogs::MessageBoxIcon::Question,
                tinyfiledialogs::YesNo::Yes
            );

            if result == tinyfiledialogs::YesNo::Yes {
                self.save_file();
            }
        }
        self.content.clear();
        self.current_file = None;
        self.unsaved_changes = false;
    }

    fn update_cursor_position(&mut self, ui: &egui::Ui, response: &egui::Response) {
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
            if let Some(cursor) = state.cursor.char_range() {
                let text_before = &self.content[..cursor.primary.index];
                let lines_before: Vec<&str> = text_before.lines().collect();
                let line = lines_before.len();
                let col = lines_before.last().map(|l| l.chars().count()).unwrap_or(0);
                self.cursor_position = (line, col);
            }
            state.store(ui.ctx(), response.id);
        }
    }

    fn create_text_edit<'a>(&'a mut self) -> egui::TextEdit<'a> {
        if self.syntax_highlighting {
            let mut job = egui::text::LayoutJob::default();

            let text = &self.content;
            let keywords = ["fn", "let", "mut", "if", "else", "for", "while", "return", "impl", "struct", "enum", "match"];
            let types = ["i32", "i64", "f32", "f64", "bool", "char", "str", "String", "Vec", "Option", "Result"];

            let mut last_pos = 0;
            let chars: Vec<char> = text.chars().collect();
            let mut i = 0;

            while i < chars.len() {
                if chars[i].is_alphabetic() || chars[i] == '_' {
                    let start = i;
                    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                        i += 1;
                    }
                    let word: String = chars[start..i].iter().collect();

                    if last_pos < start {
                        job.append(&text[last_pos..start], 0.0, egui::TextFormat::simple(
                            egui::FontId::monospace(self.zoom_level),
                            egui::Color32::WHITE
                        ));
                    }

                    let color = if keywords.contains(&&word[..]) {
                        egui::Color32::from_rgb(255, 150, 150)
                    } else if types.contains(&&word[..]) {
                        egui::Color32::from_rgb(150, 150, 255)
                    } else {
                        egui::Color32::WHITE
                    };

                    job.append(&word, 0.0, egui::TextFormat::simple(
                        egui::FontId::monospace(self.zoom_level),
                        color
                    ));

                    last_pos = i;
                } else {
                    i += 1;
                }
            }

            if last_pos < chars.len() {
                job.append(&text[last_pos..], 0.0, egui::TextFormat::simple(
                    egui::FontId::monospace(self.zoom_level),
                    egui::Color32::WHITE
                ));
            }

            egui::TextEdit::multiline(&mut self.content)
                .font(egui::FontId::monospace(self.zoom_level))
                .desired_width(f32::INFINITY)
                .desired_rows(30)
                .code_editor()
                .hint_text("Начните печатать или откройте файл...")
        } else {
            egui::TextEdit::multiline(&mut self.content)
                .id_source("main_editor")
                .font(egui::FontId::monospace(self.zoom_level))
                .desired_width(f32::INFINITY)
                .desired_rows(30)
                .code_editor()
                .hint_text("Начните печатать или откройте файл...")
        }
    }
}

impl eframe::App for TextEditor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("▶️ Run").clicked() { }
            });
        });

        if let Some(ref rx) = self.terminal_receiver {
            loop {
                match rx.try_recv() {
                    Ok(line) => self.terminal_output.push_str(&line),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.is_running = false;
                        break;
                    }
                }
            }
        }

        egui::TopBottomPanel::bottom("terminal_panel")
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Terminal Output");
                    if self.is_running {
                        ui.spinner();
                    }
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.terminal_output)
                                .font(egui::FontId::monospace(12.0))
                                .desired_width(f32::INFINITY)
                                .interactive(false)
                        );
                    });
            });

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("📁 Открыть").clicked() {
                    self.open_file();
                }
                if ui.button("💾 Сохранить").clicked() {
                    self.save_file();
                }
                if ui.button("➕ Увеличить").clicked() {
                    self.zoom_level += 2.0;
                }
                if ui.button("➖ Уменьшить").clicked() {
                    self.zoom_level = (self.zoom_level - 2.0).max(8.0);
                }
                ui.checkbox(&mut self.syntax_highlighting, "✨ Подсветка");
            });
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(path) = &self.current_file {
                    ui.label(format!("📄 {}", path.file_name().unwrap_or_default().to_string_lossy()));
                } else {
                    ui.label("📄 [Новый файл]");
                }

                ui.separator();

                if self.unsaved_changes {
                    ui.colored_label(egui::Color32::YELLOW, "● Не сохранено");
                } else {
                    ui.colored_label(egui::Color32::GREEN, "● Сохранено");
                }

                ui.separator();

                ui.label(format!("Стр: {}  Кол: {}",
                    self.cursor_position.0,
                    self.cursor_position.1
                ));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::both()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let line_count = self.content.lines().count();
                        let line_numbers_width = 50.0;

                        ui.allocate_ui_with_layout(
                            egui::vec2(line_numbers_width, ui.available_height()),
                            egui::Layout::top_down(egui::Align::RIGHT),
                            |ui| {
                                for i in 1..=line_count {
                                    ui.label(format!("{:>4}", i));
                                }
                            },
                        );

                        ui.add(egui::Separator::default().vertical());

                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), ui.available_height()),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                ui.set_min_width(400.0);

                                let text_edit = self.create_text_edit();
                                let response = ui.add(text_edit);

                                self.update_cursor_position(ui, &response);

                                if response.changed() {
                                    self.unsaved_changes = true;
                                }
                            },
                        );
                    });
                });
        });

        ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::CTRL, egui::Key::O) {
                self.open_file();
            }
            if input.consume_key(egui::Modifiers::CTRL, egui::Key::S) {
                self.save_file();
            }
            if input.consume_key(egui::Modifiers::CTRL, egui::Key::N) {
                self.new_file();
            }
            if input.consume_key(egui::Modifiers::CTRL, egui::Key::Equals) {
                self.zoom_level += 2.0;
            }
            if input.consume_key(egui::Modifiers::CTRL, egui::Key::Minus) {
                self.zoom_level = (self.zoom_level - 2.0).max(8.0);
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([600.0, 400.0])
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "Redr",
        options,
        Box::new(|_cc| Box::new(TextEditor::default())),
    )
}
