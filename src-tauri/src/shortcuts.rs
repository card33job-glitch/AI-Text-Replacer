use crate::config::{Snippet, ShortcutBinding, MENU_ACTION};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, GlobalShortcutManager};

/// Empêche qu'une seconde pression pendant un appel au modèle ne relance une
/// transformation : sans ça, un utilisateur qui ne voit rien se passer appuie
/// une deuxième fois et transforme le texte déjà remplacé.
///
/// Le contrôle spontané partage ce verrou : deux séquences de frappes simulées
/// qui s'entrelacent enverraient un Ctrl+A au milieu d'un collage.
static BUSY: AtomicBool = AtomicBool::new(false);

/// Prend le verrou, ou retourne `false` si une transformation est déjà en cours.
pub fn try_begin() -> bool {
    !BUSY.swap(true, Ordering::SeqCst)
}

pub fn end() {
    BUSY.store(false, Ordering::SeqCst);
}

/// (Ré)enregistre tous les raccourcis globaux.
///
/// Les précédents sont retirés d'abord : l'utilisateur peut changer ses
/// combinaisons depuis les Paramètres sans redémarrer l'application.
pub fn register_all(
    app: &AppHandle,
    bindings: &[ShortcutBinding],
    snippets: &[Snippet],
) -> Result<(), String> {
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

    for snippet in snippets {
        let accelerator = snippet.accelerator.trim().to_string();
        // Un texte figé sans combinaison reste dans la liste : l'utilisateur le
        // prépare peut-être, ou sa combinaison a été refusée.
        if accelerator.is_empty() || snippet.text.is_empty() {
            continue;
        }
        let handle = app.clone();
        let text = snippet.text.clone();
        if let Err(e) = manager.register(&accelerator, move || {
            on_snippet(handle.clone(), text.clone())
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
    if !try_begin() {
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
            end();
            return;
        }

        // Raccourci direct : transformation et remplacement, sans aucune UI.
        let outcome = tauri::async_runtime::block_on(crate::commands::run_action(
            app.clone(),
            text,
            action,
        ));
        end();

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

/// Insertion d'un texte figé : aucun appel au modèle, donc rien à capturer ni
/// à attendre.
fn on_snippet(app: AppHandle, text: String) {
    if !try_begin() {
        return;
    }
    crate::selection::remember_target_window();

    std::thread::spawn(move || {
        let outcome = crate::selection::insert_text(&app, text.clone());
        end();

        match outcome {
            Ok(()) => crate::toast::show_flash(&app, "Inséré"),
            // Le repli habituel : le texte est dans le presse-papiers, la popup
            // explique qu'il reste à le coller.
            Err(message) => crate::popup::show_error(&app, message, Some(text)),
        }
    });
}

/// Vérifie qu'une même combinaison n'est pas affectée à deux déclencheurs.
///
/// Les textes figés partagent l'espace des combinaisons avec les actions : les
/// deux listes sont donc contrôlées ensemble, sans quoi le second enregistrement
/// écraserait silencieusement le premier.
pub fn find_duplicate(bindings: &[ShortcutBinding], snippets: &[Snippet]) -> Option<String> {
    let mut seen: Vec<&str> = Vec::new();
    let accelerators = bindings
        .iter()
        .map(|b| b.accelerator.as_str())
        .chain(snippets.iter().map(|s| s.accelerator.as_str()));

    for accelerator in accelerators {
        let accelerator = accelerator.trim();
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
