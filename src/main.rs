#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};

use chrono::Local;
use windows::core::Result as WinResult;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds};

const CONFIG_FILE: &str = "tabs_config.json"; // Changed back to old name so we don't create an additional file

#[derive(Clone, PartialEq, Serialize, Deserialize)]
enum RetentionStrategy {
    KeepAll,
    KeepLastN(usize),
    DailyLogs,
}

#[derive(Clone, Serialize, Deserialize)]
struct AppConfig {
    retention: RetentionStrategy,
    tabs: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            retention: RetentionStrategy::KeepLastN(3000), 
            tabs: vec!["All".to_string()],
        }
    }
}

// Helper to get correct log file name based on strategy
fn get_log_file_path(strategy: &RetentionStrategy) -> PathBuf {
    match strategy {
        RetentionStrategy::DailyLogs => {
            let today = Local::now().format("%Y-%m-%d").to_string();
            PathBuf::from(format!("logs/notifications_history_{}.csv", today))
        }
        _ => PathBuf::from("logs/notifications_history.csv"),
    }
}

// Ensure the target CSV exists with headers
fn init_csv(file_path: &Path) -> io::Result<()> {
    if !file_path.exists() {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(file_path)?;
        writeln!(file, "Timestamp,App,Text")?;
    }
    Ok(())
}

fn replace_emojis(input: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let mut matched = false;
        // try to find the longest matching emoji
        for len in (1..=15).rev() {
            if i + len <= chars.len() {
                let substr: String = chars[i..i+len].iter().collect();
                if let Some(e) = emojis::get(&substr) {
                    let name = e.shortcode().unwrap_or_else(|| e.name()).replace(" ", "_").to_lowercase();
                    out.push_str(&format!(":{}:", name));
                    i += len;
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

// Create a layout job that only highlights the matched portion of the text
fn highlight_search_text(text: &str, query: &str, default_color: egui::Color32, is_app: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let base_format = egui::TextFormat {
        font_id: egui::FontId::proportional(14.0),
        color: default_color,
        ..Default::default()
    };
    
    if query.is_empty() {
        job.append(text, 0.0, base_format);
        return job;
    }
    
    let text_lower = text.to_lowercase();
    let query_lower = query.to_lowercase();
    let mut last_idx = 0;
    
    let mut highlight_format = base_format.clone();
    highlight_format.background = if is_app { egui::Color32::from_rgb(150, 100, 0) } else { egui::Color32::from_rgb(0, 80, 180) };
    highlight_format.color = egui::Color32::WHITE;

    for (idx, _) in text_lower.match_indices(&query_lower) {
        if idx > last_idx {
            job.append(&text[last_idx..idx], 0.0, base_format.clone());
        }
        job.append(&text[idx..idx + query.len()], 0.0, highlight_format.clone());
        last_idx = idx + query.len();
    }
    if last_idx < text.len() {
        job.append(&text[last_idx..], 0.0, base_format.clone());
    }
    job
}

fn log_notifications(listener: &UserNotificationListener, last_known_notifs: &mut HashSet<u32>, config: &AppConfig) -> WinResult<Vec<NotificationRecord>> {
    let notifs = listener.GetNotificationsAsync(NotificationKinds::Toast)?.get()?;
    let mut new_notifs_data = Vec::new();
    let mut current_ids = HashSet::new();

    for n in notifs {
        let notif_id = n.Id()?;
        current_ids.insert(notif_id);

        if last_known_notifs.contains(&notif_id) {
            continue;
        }

        let mut app_name = String::from("Unknown");
        if let Ok(app_info) = n.AppInfo() {
            if let Ok(display_info) = app_info.DisplayInfo() {
                if let Ok(name) = display_info.DisplayName() {
                    app_name = name.to_string_lossy();
                }
            }
        }

        let mut text_content = String::new();
        if let Ok(visual) = n.Notification()?.Visual() {
            if let Ok(binding) = visual.GetBinding(&KnownNotificationBindings::ToastGeneric()?) {
                if let Ok(text_elements) = binding.GetTextElements() {
                    let elements_vec: Vec<_> = text_elements.into_iter().collect();
                    for (i, t) in elements_vec.iter().enumerate() {
                        if let Ok(text) = t.Text() {
                            text_content.push_str(&text.to_string_lossy());
                            if i < elements_vec.len() - 1 {
                                text_content.push_str(" | ");
                            }
                        }
                    }
                }
            }
        }

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        
        // Translate emojis
        let safe_text = replace_emojis(&text_content);
        let safe_app = replace_emojis(&app_name);
        
        let escaped_text = safe_text.replace("\"", "\"\"");
        let escaped_app = safe_app.replace("\"", "\"\"");
        let csv_line = format!("{},\"{}\",\"{}\"", timestamp, escaped_app, escaped_text);
        
        let record = NotificationRecord {
            timestamp: timestamp.clone(),
            app: safe_app.clone(),
            text: safe_text.clone(),
        };
        
        new_notifs_data.push((csv_line, record));
    }

    let mut returned_records = Vec::new();
    if !new_notifs_data.is_empty() {
        new_notifs_data.reverse(); // oldest to newest for appending
        
        let log_file_path = get_log_file_path(&config.retention);
        let _ = init_csv(&log_file_path);
        
        // Append new lines
        if let Ok(mut file) = OpenOptions::new().append(true).open(&log_file_path) {
            for (line, record) in &new_notifs_data {
                if let Err(e) = writeln!(file, "{}", line) {
                    eprintln!("Error writing to file: {}", e);
                }
                returned_records.push(record.clone());
            }
        }
        
        // Handle KeepLastN pruning logic with Archive
        if let RetentionStrategy::KeepLastN(max_lines) = config.retention {
             if log_file_path.exists() {
                 if let Ok(mut rdr) = csv::ReaderBuilder::new().has_headers(true).from_path(&log_file_path) {
                     let mut all_records = Vec::new();
                     for result in rdr.records() {
                         if let Ok(rec) = result {
                             all_records.push(rec);
                         }
                     }
                     // If we exceed the limit, rewrite main file to max_lines, append rest to archive
                     if all_records.len() > max_lines {
                         let split_at = all_records.len() - max_lines;
                         
                         let pruned_records = &all_records[..split_at];
                         let kept_records = &all_records[split_at..];
                         
                         // Append pruned to archive
                         let archive_path = Path::new("logs/notifications_archive.csv");
                         let _ = init_csv(archive_path);
                         if let Ok(arc_file) = OpenOptions::new().append(true).open(archive_path) {
                             let mut arc_wtr = csv::WriterBuilder::new().has_headers(false).from_writer(arc_file);
                             for rec in pruned_records {
                                 let _ = arc_wtr.write_record(rec);
                             }
                             let _ = arc_wtr.flush();
                         }
                         
                         // Rewrite active log
                         if let Ok(mut wtr) = csv::WriterBuilder::new().from_path(&log_file_path) {
                             let _ = wtr.write_record(&["Timestamp", "App", "Text"]);
                             for rec in kept_records {
                                 let _ = wtr.write_record(rec);
                             }
                             let _ = wtr.flush();
                         }
                     }
                 }
             }
        }
    }

    *last_known_notifs = current_ids;
    Ok(returned_records)
}

async fn run_background_logger(log_history: Arc<Mutex<Vec<NotificationRecord>>>, config_arc: Arc<Mutex<AppConfig>>, ctx_repainter: egui::Context) {
    let listener = match UserNotificationListener::Current() {
        Ok(l) => l,
        Err(_) => return,
    };
    
    let access = match listener.RequestAccessAsync() {
        Ok(a) => match a.get() {
            Ok(acc) => acc,
            Err(_) => return,
        },
        Err(_) => return,
    };
    
    if access != UserNotificationListenerAccessStatus::Allowed { return; }

    let mut last_known_notifs = HashSet::new();

    loop {
        let current_config = {
            let conf_guard = config_arc.lock().unwrap();
            AppConfig { retention: conf_guard.retention.clone(), tabs: conf_guard.tabs.clone() }
        };
        
        if let Ok(new_records) = log_notifications(&listener, &mut last_known_notifs, &current_config) {
            if !new_records.is_empty() {
                if let Ok(mut hist) = log_history.lock() {
                    for r in new_records.into_iter().rev() {
                        hist.insert(0, r); // newest on top
                    }
                    if let RetentionStrategy::KeepLastN(max_n) = current_config.retention {
                        hist.truncate(max_n);
                    } else {
                         hist.truncate(50000);
                    }
                    ctx_repainter.request_repaint();
                }
            }
        }
        sleep(Duration::from_secs(1)).await;
    }
}

// --- GUI Setup ---

#[derive(Clone, Serialize, Deserialize)]
struct NotificationRecord {
    timestamp: String,
    app: String,
    text: String,
}

#[derive(PartialEq)]
enum SortOrder { Ascending, Descending }

struct LoggerApp {
    history: Arc<Mutex<Vec<NotificationRecord>>>,
    config: Arc<Mutex<AppConfig>>,
    active_tab: String,
    n_input: String,
    sort_order: SortOrder,
    search_query: String,
}

impl LoggerApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let _ = fs::create_dir_all("logs");
        
        setup_custom_fonts(&cc.egui_ctx);
        
        let mut history = Vec::new();
        
        // Load Settings
        let mut config = AppConfig::default();
        if Path::new(CONFIG_FILE).exists() {
            if let Ok(content) = fs::read_to_string(CONFIG_FILE) {
                if let Ok(parsed) = serde_json::from_str(&content) {
                    config = parsed;
                }
            }
        }
        // Ensure "All" exists
        if config.tabs.is_empty() {
             config.tabs.push("All".to_string());
        } else if config.tabs[0] != "All" {
             config.tabs.insert(0, "All".to_string());
        }
        
        let config_arc = Arc::new(Mutex::new(config.clone()));
        
        // Load history from the current active file
        let log_file = get_log_file_path(&config.retention);
        Self::load_history_from_file(&log_file, &mut history);

        let history_arc = Arc::new(Mutex::new(history));
        
        let hist_clone = Arc::clone(&history_arc);
        let conf_clone = Arc::clone(&config_arc);
        let ctx_clone = cc.egui_ctx.clone();
        
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                run_background_logger(hist_clone, conf_clone, ctx_clone).await;
            });
        });

        let mut n_in = String::from("3000");
        if let RetentionStrategy::KeepLastN(n) = config.retention {
            n_in = n.to_string();
        }

        Self {
            history: history_arc,
            config: config_arc,
            active_tab: "All".to_string(),
            n_input: n_in,
            sort_order: SortOrder::Descending, // Default Newest first
            search_query: String::new(),
        }
    }
    
    fn load_history_from_file(path: &Path, history: &mut Vec<NotificationRecord>) {
        if path.exists() {
            if let Ok(mut rdr) = csv::ReaderBuilder::new().has_headers(true).from_path(path) {
                let mut temp = Vec::new();
                for result in rdr.records() {
                    if let Ok(record) = result {
                        if record.len() >= 3 {
                            temp.push(NotificationRecord {
                                timestamp: record[0].to_string(),
                                app: record[1].to_string(),
                                text: record[2].to_string(),
                            });
                        }
                    }
                }
                temp.reverse(); // Newest first
                *history = temp;
            }
        }
    }
    
    fn save_config(&self) {
        if let Ok(guard) = self.config.lock() {
            if let Ok(json) = serde_json::to_string_pretty(&*guard) {
                let _ = fs::write(CONFIG_FILE, json);
            }
        }
    }
}

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    
    // Japanese Font (Free BIZ UDPGothic)
    let font_data = include_bytes!("bizudpgothic.ttf");
    fonts.font_data.insert(
        "bizudpgothic".to_owned(),
        egui::FontData::from_static(font_data),
    );
    
    if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        vec.insert(0, "bizudpgothic".to_owned());
    }
    if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        vec.insert(0, "bizudpgothic".to_owned());
    }
    ctx.set_fonts(fonts);
}

impl eframe::App for LoggerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut strategy_changed = false;
        let mut reload_history = false;
        let mut config_changed = false;
        
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // Apply a distinct light-blue tint to the menu area to distinguish it from log text
            ui.style_mut().visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(170, 230, 255);
            ui.style_mut().visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(130, 210, 255);

            ui.horizontal(|ui| {
                ui.heading("WinNotiCatcher");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(if self.sort_order == SortOrder::Ascending { "Sort: Oldest First ⬇" } else { "Sort: Newest First ⬆" }).clicked() {
                        self.sort_order = match self.sort_order {
                            SortOrder::Ascending => SortOrder::Descending,
                            SortOrder::Descending => SortOrder::Ascending,
                        };
                    }
                    
                    ui.add_space(10.0);
                    // Combobox for adding tabs
                    let mut app_options: Vec<String> = self.history.lock().unwrap().iter().map(|r| r.app.clone()).collect();
                    app_options.sort();
                    app_options.dedup();
                    
                    let mut selected_to_add = None;
                    egui::ComboBox::from_id_source("add_app_tab")
                        .selected_text("Add App Tab...")
                        .show_ui(ui, |ui| {
                            for app in app_options {
                                if ui.selectable_label(false, &app).clicked() {
                                    selected_to_add = Some(app);
                                }
                            }
                        });
                        
                    if let Some(app) = selected_to_add {
                        let mut conf = self.config.lock().unwrap();
                        if !conf.tabs.contains(&app) {
                            conf.tabs.push(app.clone());
                            config_changed = true;
                        }
                        self.active_tab = app;
                    }

                    if ui.button("Clear").clicked() {
                        self.search_query.clear();
                    }
                    ui.add(egui::TextEdit::singleline(&mut self.search_query).desired_width(120.0));
                    ui.label("Search Logs:");
                });
            });
            ui.separator();
            
            // TABS ROW
            ui.horizontal_wrapped(|ui| {
                let mut tabs_to_remove = Vec::new();
                
                {
                    let conf = self.config.lock().unwrap();
                    for (idx, tab) in conf.tabs.iter().enumerate() {
                        let is_active = self.active_tab == *tab;
                        
                        ui.horizontal(|ui| {
                            if ui.selectable_label(is_active, tab).clicked() {
                                self.active_tab = tab.clone();
                            }
                            
                            // Remove button (except for "All")
                            if tab != "All" {
                                if ui.small_button("x").clicked() {
                                    tabs_to_remove.push(idx);
                                }
                            }
                        });
                        ui.separator();
                    }
                    
                    // Settings tab looks like another tab but pinned right end
                    if ui.selectable_label(self.active_tab == "Settings", "⚙ Settings").clicked() {
                        self.active_tab = "Settings".to_string();
                    }
                }
                
                if !tabs_to_remove.is_empty() {
                    let mut conf = self.config.lock().unwrap();
                    for idx in tabs_to_remove.iter().rev() {
                        let removed_tab = conf.tabs.remove(*idx);
                        if self.active_tab == removed_tab {
                            self.active_tab = "All".to_string();
                        }
                    }
                    config_changed = true;
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.active_tab == "Settings" {
                // Settings Page
                let mut conf = self.config.lock().unwrap();
                ui.heading("Log Management Settings");
                ui.add_space(10.0);
                
                ui.group(|ui| {
                    ui.label("Retention Strategy (Saving format):");
                    ui.add_space(5.0);
                    if ui.radio_value(&mut conf.retention, RetentionStrategy::KeepAll, "Keep All - Uncut single history file").clicked() { strategy_changed = true; }
                    
                    let is_keep_n = matches!(conf.retention, RetentionStrategy::KeepLastN(_));
                    ui.horizontal(|ui| {
                        if ui.radio(is_keep_n, "Keep Last").clicked() {
                            if let Ok(n) = self.n_input.parse::<usize>() {
                                conf.retention = RetentionStrategy::KeepLastN(n);
                            } else {
                                conf.retention = RetentionStrategy::KeepLastN(3000);
                                self.n_input = "3000".to_string();
                            }
                            strategy_changed = true;
                        }
                        if is_keep_n {
                            let response = ui.add(egui::TextEdit::singleline(&mut self.n_input).desired_width(50.0));
                            ui.label("records. (Older records will be archived to 'notifications_archive.csv')");
                            
                            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                 if let Ok(n) = self.n_input.parse::<usize>() {
                                    conf.retention = RetentionStrategy::KeepLastN(n);
                                    strategy_changed = true;
                                }
                            }
                        }
                    });
                    
                    if ui.radio_value(&mut conf.retention, RetentionStrategy::DailyLogs, "Daily Logs - Split logs into daily files").clicked() { strategy_changed = true; }
                });
                
                if strategy_changed {
                    config_changed = true;
                    reload_history = true;
                }

                ui.add_space(20.0);
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Erase all logs from the active file:");
                        if ui.button("Clear Active Logs!").clicked() {
                            let current_file = get_log_file_path(&conf.retention);
                            if let Ok(mut file) = OpenOptions::new().write(true).truncate(true).create(true).open(&current_file) {
                                let _ = writeln!(file, "Timestamp,App,Text");
                            }
                            if let Ok(mut hist) = self.history.lock() {
                                hist.clear();
                            }
                        }
                    });
                });
            } else {
                // Log View Page
                // Enhance the background stripes contrast for better row separation
                ui.style_mut().visuals.faint_bg_color = egui::Color32::from_rgb(50, 50, 55); // make stripes slightly lighter grey to stand out more

                let query = if self.active_tab == "All" { String::new() } else { self.active_tab.to_lowercase() };
                let mut new_tab_to_add = None;
                
                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                    let mut records = self.history.lock().unwrap().clone();
                    if self.sort_order == SortOrder::Ascending {
                        records.reverse(); // Reverse makes it oldest first since memory stores newest first
                    }
                    
                    egui::Grid::new("notification_grid")
                        .striped(true)
                        .min_col_width(100.0)
                        .max_col_width(ui.available_width() - 250.0)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("Time").strong());
                            ui.label(egui::RichText::new("App Name").strong().color(egui::Color32::LIGHT_BLUE));
                            ui.label(egui::RichText::new("Notification Text").strong());
                            ui.end_row();

                            let search_q = self.search_query.to_lowercase();
                            
                            for record in records {
                                if !query.is_empty() && record.app.to_lowercase() != query {
                                    continue;
                                }
                                
                                let app_lower = record.app.to_lowercase();
                                let text_lower = record.text.to_lowercase();
                                
                                // Search filtering
                                if !search_q.is_empty() && !app_lower.contains(&search_q) && !text_lower.contains(&search_q) {
                                    continue;
                                }
                                
                                // Top align the content by wrapping in a top_down layout
                                ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                                    ui.add_space(3.0);
                                    ui.label(&record.timestamp);
                                });
                                
                                // App Label Highlight using LayoutJob
                                let app_job = highlight_search_text(&record.app, &search_q, ui.visuals().hyperlink_color, true);
                                
                                // Make App Label clickable to create new tab top-aligned
                                ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                                    ui.add_space(3.0);
                                    if ui.add(egui::Label::new(app_job).sense(egui::Sense::click())).clicked() {
                                         new_tab_to_add = Some(record.app.clone());
                                    }
                                });
                                
                                // Text Label Highlight using LayoutJob
                                let text_job = highlight_search_text(&record.text, &search_q, ui.visuals().text_color(), false);
                                
                                ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                                    ui.add_space(3.0);
                                    ui.add(egui::Label::new(text_job).wrap(true));
                                    ui.add_space(3.0);
                                });
                                ui.end_row();
                            }
                    });
                });
                
                if let Some(app_name) = new_tab_to_add {
                    let mut conf = self.config.lock().unwrap();
                    if !conf.tabs.contains(&app_name) {
                        conf.tabs.push(app_name.clone());
                        config_changed = true;
                    }
                    self.active_tab = app_name;
                }
            }
        });

        if config_changed {
            self.save_config();
        }
        
        if reload_history {
            let current_file = {
                 let conf = self.config.lock().unwrap();
                 get_log_file_path(&conf.retention)
            };
            let mut temp = Vec::new();
            Self::load_history_from_file(&current_file, &mut temp);
            if let Ok(mut hist) = self.history.lock() {
                *hist = temp;
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 600.0])
            .with_title("WinNotiCatcher"),
        ..Default::default()
    };
    
    eframe::run_native(
        "WinNotiCatcher",
        options,
        Box::new(|cc| Box::new(LoggerApp::new(cc))),
    )
}
