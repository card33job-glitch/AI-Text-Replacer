#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod ai;
mod autostart;
mod clipboard;
mod commands;
mod config;
mod permissions;
mod popup;
mod proactive;
mod selection;
mod shortcuts;
mod toast;

use tauri::{
    CustomMenuItem, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu, SystemTrayMenuItem,
    WindowEvent,
};

fn tray() -> SystemTray {
    let menu = SystemTrayMenu::new()
        .add_item(CustomMenuItem::new("open", "Ouvrir AI Text Replacer"))
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(CustomMenuItem::new("quit", "Quitter"));
    SystemTray::new().with_menu(menu)
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn main() {
    tauri::Builder::default()
        .system_tray(tray())
        .on_system_tray_event(|app, event| match event {
            SystemTrayEvent::LeftClick { .. } => show_main(app),
            SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                "open" => show_main(app),
                "quit" => {
                    shortcuts::unregister_all(app);
                    app.exit(0);
                }
                _ => {}
            },
            _ => {}
        })
        .on_window_event(|event| match event.event() {
            // Fermer la fenêtre principale met l'application en veille dans la
            // zone de notification : le raccourci global doit rester actif.
            WindowEvent::CloseRequested { api, .. } if event.window().label() == "main" => {
                api.prevent_close();
                let _ = event.window().hide();
            }
            // Cliquer ailleurs referme la popup, comme un vrai menu contextuel.
            WindowEvent::Focused(false) if event.window().label() == popup::POPUP_LABEL => {
                let _ = event.window().hide();
            }
            _ => {}
        })
        .setup(|app| {
            let handle = app.handle();
            let cfg = config::get(&handle);

            if let Err(e) = shortcuts::register_all(&handle, &cfg.shortcuts, &cfg.snippets) {
                eprintln!("Raccourcis globaux: {}", e);
            }

            // L'observateur tourne en permanence : même mode proactif éteint,
            // il retient la dernière application active pour les Paramètres.
            proactive::start(handle.clone());

            // Remet l'entrée de démarrage en phase avec la config : elle pointe
            // peut-être encore vers l'emplacement d'avant une mise à jour.
            if let Err(e) = autostart::sync(cfg.start_at_login) {
                eprintln!("Démarrage automatique: {}", e);
            }

            // Ouvrir une fenêtre à chaque ouverture de session serait une
            // nuisance : lancée automatiquement, l'application reste dans la
            // zone de notification, prête à répondre aux raccourcis.
            if cfg.start_minimized || autostart::launched_at_login() {
                if let Some(window) = app.get_window("main") {
                    let _ = window.hide();
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_clipboard,
            commands::set_clipboard,
            commands::get_config,
            commands::save_config,
            commands::get_pending_selection,
            commands::capture_selection,
            commands::transform_text,
            commands::transform_and_replace,
            commands::replace_selection,
            commands::hide_popup,
            commands::open_main_window,
            commands::accessibility_status,
            commands::last_foreground_app,
            commands::get_history,
            commands::clear_history,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
