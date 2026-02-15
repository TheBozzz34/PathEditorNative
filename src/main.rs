#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("This application is Windows-only.");
}

#[cfg(target_os = "windows")]
mod app {
    use std::collections::{BTreeSet, HashSet};
    use std::env;
    use std::error::Error;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::process;

    use eframe::egui::{self, Color32, RichText, ScrollArea, TextEdit};
    use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, SW_SHOW, WM_SETTINGCHANGE,
    };
    use winreg::enums::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ,
        RegType,
    };
    use winreg::{HKEY, RegKey, RegValue};

    const USER_ENV_KEY: &str = "Environment";
    const SYSTEM_ENV_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

    pub fn run() -> eframe::Result<()> {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1220.0, 760.0]),
            ..Default::default()
        };

        eframe::run_native(
            "PATH Editor Native",
            native_options,
            Box::new(|cc| Box::new(PathEditorApp::new(cc))),
        )
    }

    #[derive(Clone)]
    struct PathStore {
        parts: Vec<String>,
        filter: String,
        selected: BTreeSet<usize>,
        reg_type: RegType,
    }

    impl PathStore {
        fn new(raw: String, reg_type: RegType) -> Self {
            Self {
                parts: split_path(&raw),
                filter: String::new(),
                selected: BTreeSet::new(),
                reg_type,
            }
        }

        fn visible_indices(&self) -> Vec<usize> {
            let filter = self.filter.trim().to_lowercase();
            self.parts
                .iter()
                .enumerate()
                .filter_map(|(idx, part)| {
                    if filter.is_empty() || part.to_lowercase().contains(&filter) {
                        Some(idx)
                    } else {
                        None
                    }
                })
                .collect()
        }

        fn raw_preview(&self) -> String {
            join_path(&self.parts)
        }
    }

    struct AddDialogState {
        open: bool,
        is_system: bool,
        input: String,
    }

    impl Default for AddDialogState {
        fn default() -> Self {
            Self {
                open: false,
                is_system: false,
                input: String::new(),
            }
        }
    }

    #[derive(Default)]
    struct ExpandedDialogState {
        open: bool,
        is_system: bool,
    }

    struct PathEditorApp {
        user: PathStore,
        system: PathStore,
        status: String,
        is_admin: bool,
        add_dialog: AddDialogState,
        expanded_dialog: ExpandedDialogState,
    }

    impl PathEditorApp {
        fn new(cc: &eframe::CreationContext<'_>) -> Self {
            apply_style(&cc.egui_ctx);

            let (user_raw, user_type) =
                read_reg_value(HKEY_CURRENT_USER, USER_ENV_KEY, "Path").unwrap_or_else(|_| {
                    (String::new(), REG_SZ)
                });
            let (system_raw, system_type) =
                read_reg_value(HKEY_LOCAL_MACHINE, SYSTEM_ENV_KEY, "Path").unwrap_or_else(|_| {
                    (String::new(), REG_SZ)
                });

            Self {
                user: PathStore::new(user_raw, user_type),
                system: PathStore::new(system_raw, system_type),
                status: "Ready".to_string(),
                is_admin: is_admin(),
                add_dialog: AddDialogState::default(),
                expanded_dialog: ExpandedDialogState::default(),
            }
        }

        fn panel_title(is_system: bool) -> &'static str {
            if is_system {
                "System PATH (HKLM)"
            } else {
                "User PATH (HKCU)"
            }
        }

        fn open_add_dialog(&mut self, is_system: bool) {
            self.add_dialog.open = true;
            self.add_dialog.is_system = is_system;
            self.add_dialog.input.clear();
        }

        fn open_expanded_dialog(&mut self, is_system: bool) {
            self.expanded_dialog.open = true;
            self.expanded_dialog.is_system = is_system;
        }

        fn store(&self, is_system: bool) -> &PathStore {
            if is_system {
                &self.system
            } else {
                &self.user
            }
        }

        fn store_mut(&mut self, is_system: bool) -> &mut PathStore {
            if is_system {
                &mut self.system
            } else {
                &mut self.user
            }
        }

        fn remove_selected(&mut self, is_system: bool) {
            let store = self.store_mut(is_system);
            let before = store.parts.len();
            store.parts = store
                .parts
                .iter()
                .enumerate()
                .filter_map(|(idx, part)| {
                    if store.selected.contains(&idx) {
                        None
                    } else {
                        Some(part.clone())
                    }
                })
                .collect();
            let removed = before.saturating_sub(store.parts.len());
            store.selected.clear();
            self.status = format!(
                "Removed {removed} {} entry/entries",
                Self::panel_title(is_system)
            );
        }

        fn move_selected(&mut self, is_system: bool, direction: i32) {
            let store = self.store_mut(is_system);
            if !store.filter.trim().is_empty() {
                MessageDialog::new()
                    .set_level(MessageLevel::Info)
                    .set_title("Move disabled while filtering")
                    .set_description("Clear the filter before moving entries.")
                    .set_buttons(MessageButtons::Ok)
                    .show();
                return;
            }

            let old = store.selected.clone();
            if old.is_empty() {
                return;
            }

            let mut new_selected = old.clone();
            if direction < 0 {
                for idx in old.iter().copied() {
                    if idx > 0 && !old.contains(&(idx - 1)) {
                        store.parts.swap(idx, idx - 1);
                        new_selected.remove(&idx);
                        new_selected.insert(idx - 1);
                    }
                }
            } else {
                for idx in old.iter().copied().rev() {
                    if idx + 1 < store.parts.len() && !old.contains(&(idx + 1)) {
                        store.parts.swap(idx, idx + 1);
                        new_selected.remove(&idx);
                        new_selected.insert(idx + 1);
                    }
                }
            }
            store.selected = new_selected;
            self.status = format!("Reordered {}", Self::panel_title(is_system));
        }

        fn apply_dedupe(&mut self, is_system: bool) {
            let store = self.store_mut(is_system);
            let before = store.parts.len();
            store.parts = dedupe(&store.parts);
            store.selected.clear();
            self.status = format!(
                "Dedupe removed {} entries from {}",
                before.saturating_sub(store.parts.len()),
                Self::panel_title(is_system)
            );
        }

        fn apply_sort(&mut self, is_system: bool) {
            let store = self.store_mut(is_system);
            sort_case_insensitive(&mut store.parts);
            store.selected.clear();
            self.status = format!("Sorted {}", Self::panel_title(is_system));
        }

        fn save_one(&mut self, is_system: bool) {
            if is_system && !self.is_admin {
                MessageDialog::new()
                    .set_level(MessageLevel::Error)
                    .set_title("Administrator required")
                    .set_description("Saving System PATH requires running as Administrator.")
                    .set_buttons(MessageButtons::Ok)
                    .show();
                return;
            }

            match self.write_path(is_system) {
                Ok(()) => {
                    let target = if is_system { "System" } else { "User" };
                    self.status = format!("Saved {target} PATH and broadcasted change");
                    MessageDialog::new()
                        .set_level(MessageLevel::Info)
                        .set_title("Saved")
                        .set_description(format!(
                            "{target} PATH saved. New terminals/apps will see the change."
                        ))
                        .set_buttons(MessageButtons::Ok)
                        .show();
                }
                Err(err) => {
                    MessageDialog::new()
                        .set_level(MessageLevel::Error)
                        .set_title("Save failed")
                        .set_description(err.to_string())
                        .set_buttons(MessageButtons::Ok)
                        .show();
                }
            }
        }

        fn save_all(&mut self) {
            if let Err(err) = self.write_path(false) {
                MessageDialog::new()
                    .set_level(MessageLevel::Error)
                    .set_title("Save failed")
                    .set_description(err.to_string())
                    .set_buttons(MessageButtons::Ok)
                    .show();
                return;
            }

            if self.is_admin {
                if let Err(err) = self.write_path(true) {
                    MessageDialog::new()
                        .set_level(MessageLevel::Error)
                        .set_title("Save failed")
                        .set_description(err.to_string())
                        .set_buttons(MessageButtons::Ok)
                        .show();
                    return;
                }
                self.status = "Saved User + System PATH and broadcasted change".to_string();
                MessageDialog::new()
                    .set_level(MessageLevel::Info)
                    .set_title("Saved")
                    .set_description("User and System PATH saved.")
                    .set_buttons(MessageButtons::Ok)
                    .show();
            } else {
                self.status = "Saved User PATH (System PATH skipped - not admin)".to_string();
                MessageDialog::new()
                    .set_level(MessageLevel::Info)
                    .set_title("Saved")
                    .set_description("User PATH saved. System PATH was skipped because this process is not elevated.")
                    .set_buttons(MessageButtons::Ok)
                    .show();
            }
        }

        fn write_path(&mut self, is_system: bool) -> Result<(), Box<dyn Error>> {
            let store = self.store(is_system);
            let value = join_path(&store.parts);
            let mut vtype = store.reg_type.clone();

            if has_env_token(&value) {
                vtype = REG_EXPAND_SZ;
            } else if vtype != REG_SZ && vtype != REG_EXPAND_SZ {
                vtype = REG_SZ;
            }

            if is_system {
                write_reg_value(
                    HKEY_LOCAL_MACHINE,
                    SYSTEM_ENV_KEY,
                    "Path",
                    &value,
                    vtype.clone(),
                )?;
                self.system.reg_type = vtype;
            } else {
                write_reg_value(
                    HKEY_CURRENT_USER,
                    USER_ENV_KEY,
                    "Path",
                    &value,
                    vtype.clone(),
                )?;
                self.user.reg_type = vtype;
            }

            broadcast_env_change();

            if is_system {
                let merged = join_path(
                    &self
                        .user
                        .parts
                        .iter()
                        .chain(self.system.parts.iter())
                        .cloned()
                        .collect::<Vec<_>>(),
                );
                env::set_var("PATH", merged);
            } else {
                env::set_var("PATH", value);
            }

            Ok(())
        }

        fn draw_panel(&mut self, ui: &mut egui::Ui, is_system: bool) {
            let mut do_add = false;
            let mut do_browse = false;
            let mut do_remove = false;
            let mut do_up = false;
            let mut do_down = false;
            let mut do_dedupe = false;
            let mut do_sort = false;
            let mut do_expand = false;
            let mut do_save = false;

            {
                let store = self.store_mut(is_system);

                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.heading(Self::panel_title(is_system));
                        ui.label(
                            RichText::new("Use filter + multiselect (Ctrl+Click) to edit entries quickly.")
                                .small()
                                .color(Color32::from_gray(170)),
                        );
                        ui.add_space(8.0);

                        ui.horizontal(|ui| {
                            ui.label("Filter");
                            ui.add(
                                TextEdit::singleline(&mut store.filter)
                                    .hint_text("Type to filter PATH entries")
                                    .desired_width(f32::INFINITY),
                            );
                        });

                        ui.add_space(8.0);

                        let visible = store.visible_indices();

                        egui::Frame::canvas(ui.style()).show(ui, |ui| {
                            ui.set_height(300.0);
                            ScrollArea::vertical()
                                .id_source(format!("list_{is_system}"))
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for idx in visible {
                                        let selected = store.selected.contains(&idx);
                                        let response = ui.selectable_label(selected, &store.parts[idx]);
                                        if response.clicked() {
                                            let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
                                            if ctrl {
                                                if selected {
                                                    store.selected.remove(&idx);
                                                } else {
                                                    store.selected.insert(idx);
                                                }
                                            } else {
                                                store.selected.clear();
                                                store.selected.insert(idx);
                                            }
                                        }
                                    }
                                });
                        });

                        ui.add_space(8.0);

                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Add").clicked() {
                                do_add = true;
                            }
                            if ui.button("Browse").clicked() {
                                do_browse = true;
                            }
                            if ui.button("Remove").clicked() {
                                do_remove = true;
                            }
                            if ui
                                .add_enabled(
                                    store.filter.trim().is_empty(),
                                    egui::Button::new("Move Up"),
                                )
                                .clicked()
                            {
                                do_up = true;
                            }
                            if ui
                                .add_enabled(
                                    store.filter.trim().is_empty(),
                                    egui::Button::new("Move Down"),
                                )
                                .clicked()
                            {
                                do_down = true;
                            }
                            if ui.button("Dedupe").clicked() {
                                do_dedupe = true;
                            }
                            if ui.button("Sort").clicked() {
                                do_sort = true;
                            }
                            if ui.button("Expanded").clicked() {
                                do_expand = true;
                            }
                        });

                        ui.add_space(8.0);
                        ui.label(RichText::new("Raw PATH preview").small());
                        let mut raw_preview = store.raw_preview();
                        ui.add(
                            TextEdit::multiline(&mut raw_preview)
                                .desired_rows(5)
                                .interactive(false)
                                .desired_width(f32::INFINITY),
                        );

                        ui.add_space(8.0);
                        if ui.button("Save this PATH").clicked() {
                            do_save = true;
                        }
                    });
                });
            }

            if do_add {
                self.open_add_dialog(is_system);
            }
            if do_browse {
                if let Some(folder) = FileDialog::new().pick_folder() {
                    self.store_mut(is_system)
                        .parts
                        .push(folder.display().to_string());
                    self.status = format!("Added folder to {}", Self::panel_title(is_system));
                }
            }
            if do_remove {
                self.remove_selected(is_system);
            }
            if do_up {
                self.move_selected(is_system, -1);
            }
            if do_down {
                self.move_selected(is_system, 1);
            }
            if do_dedupe {
                self.apply_dedupe(is_system);
            }
            if do_sort {
                self.apply_sort(is_system);
            }
            if do_expand {
                self.open_expanded_dialog(is_system);
            }
            if do_save {
                self.save_one(is_system);
            }
        }

        fn draw_add_dialog(&mut self, ctx: &egui::Context) {
            if !self.add_dialog.open {
                return;
            }

            let mut open = self.add_dialog.open;
            let title = if self.add_dialog.is_system {
                "Add System PATH Entry"
            } else {
                "Add User PATH Entry"
            };

            egui::Window::new(title)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(640.0)
                .show(ctx, |ui| {
                    ui.label("Entry (supports %VAR% tokens, e.g. %SystemRoot%\\System32)");
                    ui.add_space(6.0);
                    ui.add(
                        TextEdit::singleline(&mut self.add_dialog.input)
                            .desired_width(f32::INFINITY)
                            .hint_text(r"C:\Tools\bin"),
                    );

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Browse...").clicked() {
                            if let Some(folder) = FileDialog::new().pick_folder() {
                                self.add_dialog.input = folder.display().to_string();
                            }
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Add").clicked() {
                                let v = self.add_dialog.input.trim().to_string();
                                if !v.is_empty() {
                                    self.store_mut(self.add_dialog.is_system).parts.push(v);
                                    self.status = format!(
                                        "Added entry to {}",
                                        Self::panel_title(self.add_dialog.is_system)
                                    );
                                }
                                self.add_dialog.input.clear();
                                self.add_dialog.open = false;
                            }
                            if ui.button("Cancel").clicked() {
                                self.add_dialog.open = false;
                                self.add_dialog.input.clear();
                            }
                        });
                    });
                });

            self.add_dialog.open = open;
        }

        fn draw_expanded_dialog(&mut self, ctx: &egui::Context) {
            if !self.expanded_dialog.open {
                return;
            }

            let mut open = self.expanded_dialog.open;
            let is_system = self.expanded_dialog.is_system;
            let title = if is_system {
                "Expanded System PATH"
            } else {
                "Expanded User PATH"
            };
            let content = self
                .store(is_system)
                .parts
                .iter()
                .map(|p| format!("{p}\n    -> {}", expand_env_vars(p)))
                .collect::<Vec<_>>()
                .join("\n\n");

            let mut content_mut = content;
            egui::Window::new(title)
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_size([980.0, 420.0])
                .show(ctx, |ui| {
                    ui.add(
                        TextEdit::multiline(&mut content_mut)
                            .desired_width(f32::INFINITY)
                            .desired_rows(24)
                            .interactive(false),
                    );
                });

            self.expanded_dialog.open = open;
        }

        fn restart_elevated(&mut self) {
            match restart_as_admin() {
                Ok(()) => {
                    process::exit(0);
                }
                Err(err) => {
                    MessageDialog::new()
                        .set_level(MessageLevel::Error)
                        .set_title("Failed to restart as Administrator")
                        .set_description(err.to_string())
                        .set_buttons(MessageButtons::Ok)
                        .show();
                }
            }
        }
    }

    impl eframe::App for PathEditorApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            egui::TopBottomPanel::top("header").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("PATH Editor Native");
                    ui.separator();
                    ui.label("Directly edits registry PATH values (User and System).");
                    if !self.is_admin {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(
                                    RichText::new("Restart as Admin")
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .clicked()
                            {
                                self.restart_elevated();
                            }
                        });
                    }
                });
            });

            egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.status).small());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(RichText::new("Save ALL").strong().color(Color32::WHITE))
                            .clicked()
                        {
                            self.save_all();
                        }
                    });
                });
            });

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.columns(2, |cols| {
                    self.draw_panel(&mut cols[0], false);
                    self.draw_panel(&mut cols[1], true);
                });
            });

            self.draw_add_dialog(ctx);
            self.draw_expanded_dialog(ctx);
        }
    }

    fn apply_style(ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.visuals = egui::Visuals::dark();
        style.visuals.window_fill = Color32::from_rgb(20, 24, 30);
        style.visuals.panel_fill = Color32::from_rgb(17, 20, 26);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(0, 120, 212);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(0, 96, 172);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(37, 44, 54);
        style.visuals.hyperlink_color = Color32::from_rgb(0, 153, 255);
        ctx.set_style(style);
    }

    fn split_path(path: &str) -> Vec<String> {
        path.split(';')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }

    fn join_path(parts: &[String]) -> String {
        parts.join(";")
    }

    fn expand_env_vars(input: &str) -> String {
        let chars: Vec<char> = input.chars().collect();
        let mut out = String::with_capacity(input.len());
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '%' {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '%' {
                    j += 1;
                }
                if j < chars.len() && j > i + 1 {
                    let name: String = chars[i + 1..j].iter().collect();
                    match env::var(&name) {
                        Ok(value) => out.push_str(&value),
                        Err(_) => {
                            out.push('%');
                            out.push_str(&name);
                            out.push('%');
                        }
                    }
                    i = j + 1;
                    continue;
                }
            }

            out.push(chars[i]);
            i += 1;
        }

        out
    }

    fn normalize_for_compare(path: &str) -> String {
        let mut normalized = expand_env_vars(path).replace('/', "\\").trim().to_lowercase();
        while normalized.ends_with('\\') {
            normalized.pop();
        }
        normalized
    }

    fn dedupe(parts: &[String]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut out = Vec::with_capacity(parts.len());
        for part in parts {
            let key = normalize_for_compare(part);
            if seen.insert(key) {
                out.push(part.clone());
            }
        }
        out
    }

    fn sort_case_insensitive(parts: &mut [String]) {
        parts.sort_by_cached_key(|p| p.to_lowercase());
    }

    fn has_env_token(value: &str) -> bool {
        let chars: Vec<char> = value.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '%' {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '%' {
                    j += 1;
                }
                if j < chars.len() && j > i + 1 {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    fn is_admin() -> bool {
        unsafe { IsUserAnAdmin().as_bool() }
    }

    fn restart_as_admin() -> Result<(), Box<dyn Error>> {
        let exe = env::current_exe()?;
        let exe_str = exe.to_string_lossy().to_string();
        let args = env::args()
            .skip(1)
            .map(|a| quote_arg(&a))
            .collect::<Vec<_>>()
            .join(" ");

        let op = to_wide("runas");
        let exe_w = to_wide(&exe_str);
        let args_w = to_wide(&args);

        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(op.as_ptr()),
                PCWSTR(exe_w.as_ptr()),
                if args.is_empty() {
                    PCWSTR::null()
                } else {
                    PCWSTR(args_w.as_ptr())
                },
                PCWSTR::null(),
                SW_SHOW,
            )
        };

        if result.0 as isize <= 32 {
            Err(format!("ShellExecuteW failed with code {}", result.0 as isize).into())
        } else {
            Ok(())
        }
    }

    fn quote_arg(arg: &str) -> String {
        if arg.contains(' ') || arg.contains('"') {
            format!("\"{}\"", arg.replace('"', "\\\""))
        } else {
            arg.to_string()
        }
    }

    fn broadcast_env_change() {
        let env = to_wide("Environment");
        let mut result = 0usize;
        unsafe {
            let _ = SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(env.as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                2000,
                Some(&mut result),
            );
        }
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    fn read_reg_value(
        root: HKEY,
        subkey: &str,
        name: &str,
    ) -> Result<(String, RegType), Box<dyn Error>> {
        let key = RegKey::predef(root).open_subkey_with_flags(subkey, KEY_READ)?;
        match key.get_raw_value(name) {
            Ok(raw) => Ok((decode_utf16_reg(&raw.bytes), raw.vtype)),
            Err(_) => Ok((String::new(), REG_SZ)),
        }
    }

    fn write_reg_value(
        root: HKEY,
        subkey: &str,
        name: &str,
        value: &str,
        vtype: RegType,
    ) -> Result<(), Box<dyn Error>> {
        let key = RegKey::predef(root).open_subkey_with_flags(subkey, KEY_SET_VALUE)?;
        let raw = RegValue {
            bytes: encode_utf16_reg(value),
            vtype,
        };
        key.set_raw_value(name, &raw)?;
        Ok(())
    }

    fn decode_utf16_reg(bytes: &[u8]) -> String {
        if bytes.len() < 2 {
            return String::new();
        }

        let mut utf16 = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks_exact(2) {
            utf16.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }

        while utf16.last() == Some(&0) {
            utf16.pop();
        }

        String::from_utf16_lossy(&utf16)
    }

    fn encode_utf16_reg(value: &str) -> Vec<u8> {
        value
            .encode_utf16()
            .chain(Some(0))
            .flat_map(|u| u.to_le_bytes())
            .collect()
    }

    #[allow(dead_code)]
    fn confirm_overwrite() -> bool {
        matches!(
            MessageDialog::new()
                .set_level(MessageLevel::Info)
                .set_title("Confirm")
                .set_description("Apply PATH changes?")
                .set_buttons(MessageButtons::OkCancel)
                .show(),
            MessageDialogResult::Ok
        )
    }
}

#[cfg(target_os = "windows")]
fn main() -> eframe::Result<()> {
    app::run()
}
