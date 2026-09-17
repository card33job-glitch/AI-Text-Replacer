use crate::config::{ShortcutBinding, MENU_ACTION};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, GlobalShortcutManager};

/// Empêche qu'une seconde pression pendant un appel au modèle ne relance une
/// transformation : sans ça, un utilisateur qui ne voit rien se passer appuie
/// une deuxième fois et transforme le texte déjà remplacé.
static BUSY: AtomicBool = AtomicBool::new(false);

/// (Ré)enregistre tous les raccourcis globaux.
///
/// Les précédents sont retirés d'abord : l'utilisateur peut changer ses
/// combinaisons depuis les Paramètres sans redémarrer l'application.
pub fn register_all(app: &AppHandle, bindings: &[ShortcutBinding]) -> Result<(), String> {
    let mut manager = app.global_shortcut_manager();
    let _ = manager.unregister_all();

    let mut failures = Vec::new();
    for binding in bindings {
        let accelerator = binding.accelerator.trim().to_string();
        if accelerator.is_empty() {
            continue;
        }
        let handle = app.clone();
        let action = binding.action.clone();
        if let Err(e) = manager.register(&accelerator, move || {
            on_trigger(handle.clone(), action.clone())
        }) {
            failures.push(format!("{} ({})", accelerator, e));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Raccourci(s) refusé(s) : {}. Ils sont probablement déjà utilisés par une autre application.",
            failures.join(", ")
        ))
    }
}

pub fn unregister_all(app: &AppHandle) {
    let _ = app.global_shortcut_manager().unregister_all();
}

/// Déclenché à chaque pression d'un raccourci global.
fn on_trigger(app: AppHandle, action: String) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }

    // La fenêtre cible doit être relevée immédiatement : dès que notre popup
    // s'affichera, c'est elle qui sera au premier plan.
    crate::selection::remember_target_window();

    // La capture enchaîne des attentes ; la garder hors du thread du
    // gestionnaire de raccourcis évite de bloquer les pressions suivantes.
    std::thread::spawn(move || {
        let text = crate::selection::capture_selection(&app).unwrap_or_default();

        if action == MENU_ACTION || text.trim().is_empty() {
            // Rien de sélectionné : la popup explique quoi faire, même pour un
            // raccourci direct.
            if let Err(e) = crate::popup::show(&app, text) {
                eprintln!("Affichage de la popup impossible: {}", e);
            }
            BUSY.store(false, Ordering::SeqCst);
            return;
        }

        // Raccourci direct : transformation et remplacement, sans aucune UI.
        let outcome = tauri::async_runtime::block_on(crate::commands::run_action(
            app.clone(),
            text,
            action,
        ));
        BUSY.store(false, Ordering::SeqCst);

        match outcome {
            Ok(result) if result.replaced => {}
            Ok(result) => crate::popup::show_error(
                &app,
                result
                    .message
                    .unwrap_or_else(|| "Le texte n'a pas pu être remplacé.".to_string()),
                Some(result.text),
            ),
            Err(message) => crate::popup::show_error(&app, message, None),
        }
    });
}

/// Vérifie qu'une même combinaison n'est pas affectée à deux actions.
pub fn find_duplicate(bindings: &[ShortcutBinding]) -> Option<String> {
    let mut seen: Vec<&str> = Vec::new();
    for binding in bindings {
        let accelerator = binding.accelerator.trim();
        if accelerator.is_empty() {
            continue;
        }
        if seen.iter().any(|s| s.eq_ignore_ascii_case(accelerator)) {
            return Some(accelerator.to_string());
        }
        seen.push(accelerator);
    }
    None
}
