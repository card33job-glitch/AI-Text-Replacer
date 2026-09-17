use crate::config::{self, AppConfig, HistoryEntry};
use crate::popup::{self, Selection};
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformOutcome {
    /// Texte produit par le modèle, toujours renvoyé même si le collage échoue.
    pub text: String,
    pub replaced: bool,
    pub message: Option<String>,
}

#[tauri::command]
pub async fn get_clipboard(app: AppHandle) -> Result<String, String> {
    crate::clipboard::get_clipboard(&app)
}

#[tauri::command]
pub async fn set_clipboard(app: AppHandle, text: String) -> Result<(), String> {
    crate::clipboard::set_clipboard(&app, text)
}

#[tauri::command]
pub async fn get_config(app: AppHandle) -> Result<AppConfig, String> {
    Ok(config::get(&app))
}

/// Sauvegarde les paramètres et applique immédiatement les raccourcis.
#[tauri::command]
pub async fn save_config(app: AppHandle, config: AppConfig) -> Result<(), String> {
    const CAPTURE_MODES: [&str; 3] = [
        config::CAPTURE_FIELD,
        config::CAPTURE_SELECTION,
        config::CAPTURE_SELECTION_THEN_FIELD,
    ];
    if !CAPTURE_MODES.contains(&config.capture_mode.as_str()) {
        return Err(format!(
            "Mode de capture inconnu : « {} ».",
            config.capture_mode
        ));
    }

    if let Some(duplicate) = crate::shortcuts::find_duplicate(&config.shortcuts) {
        return Err(format!(
            "« {} » est affecté à deux actions. Chaque raccourci doit être unique.",
            duplicate
        ));
    }

    let previous = config::get(&app);
    let shortcuts = config.shortcuts.clone();
    config::save(&app, config)?;

    if let Err(e) = crate::shortcuts::register_all(&app, &shortcuts) {
        // Une combinaison est refusée (déjà prise par une autre application) :
        // on remet les précédentes pour ne pas laisser l'utilisateur sans
        // aucun raccourci actif.
        let _ = crate::shortcuts::register_all(&app, &previous.shortcuts);
        let mut rollback = config::get(&app);
        rollback.shortcuts = previous.shortcuts;
        let _ = config::save(&app, rollback);
        return Err(e);
    }
    Ok(())
}

#[tauri::command]
pub async fn get_pending_selection(app: AppHandle) -> Result<Selection, String> {
    Ok(popup::pending_selection(&app))
}

/// Copie la sélection courante de l'application active (bouton « Capturer »).
#[tauri::command]
pub async fn capture_selection(app: AppHandle) -> Result<String, String> {
    crate::selection::remember_target_window();
    tauri::async_runtime::spawn_blocking(move || crate::selection::capture_selection(&app))
        .await
        .map_err(|e| format!("Capture interrompue: {}", e))?
}

/// Transforme du texte sans rien remplacer (panneau principal, ou aperçu).
#[tauri::command]
pub async fn transform_text(
    app: AppHandle,
    text: String,
    action: String,
    provider: Option<String>,
) -> Result<String, String> {
    let cfg = config::get(&app);
    let provider = provider
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| cfg.default_provider.clone());
    let result = crate::ai::transform(&cfg, &provider, &action, &text).await?;

    let _ = config::push_history(
        &app,
        HistoryEntry {
            id: format!("{}", config::now_millis()),
            original: text,
            transformed: result.clone(),
            action,
            provider,
            timestamp: config::now_millis(),
        },
    );
    Ok(result)
}

/// Chaîne complète : transformation puis remplacement de la sélection dans
/// l'application d'origine (Teams, Outlook, navigateur…).
#[tauri::command]
pub async fn transform_and_replace(
    app: AppHandle,
    text: String,
    action: String,
    provider: Option<String>,
) -> Result<TransformOutcome, String> {
    run_action_with(app, text, action, provider).await
}

/// Même chaîne, appelée depuis un raccourci direct (sans passer par le menu).
pub async fn run_action(
    app: AppHandle,
    text: String,
    action: String,
) -> Result<TransformOutcome, String> {
    run_action_with(app, text, action, None).await
}

async fn run_action_with(
    app: AppHandle,
    text: String,
    action: String,
    provider: Option<String>,
) -> Result<TransformOutcome, String> {
    let cfg = config::get(&app);
    let provider = provider
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| cfg.default_provider.clone());

    crate::toast::show_analyzing(&app);
    let result = match crate::ai::transform(&cfg, &provider, &action, &text).await {
        Ok(result) => result,
        Err(e) => {
            // La popup prend le relais pour expliquer l'échec.
            crate::toast::hide(&app);
            return Err(e);
        }
    };

    let done_label = crate::toast::done_label(&action);
    let _ = config::push_history(
        &app,
        HistoryEntry {
            id: format!("{}", config::now_millis()),
            original: text,
            transformed: result.clone(),
            action,
            provider,
            timestamp: config::now_millis(),
        },
    );

    replace(app, result, done_label).await
}

/// Colle un texte déjà transformé (utilisé après l'aperçu).
#[tauri::command]
pub async fn replace_selection(app: AppHandle, text: String) -> Result<TransformOutcome, String> {
    replace(app, text, "Remplacé").await
}

async fn replace(
    app: AppHandle,
    text: String,
    done_label: &str,
) -> Result<TransformOutcome, String> {
    popup::hide(&app);

    let handle = app.clone();
    let payload = text.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        crate::selection::replace_selection(&handle, payload)
    })
    .await
    .map_err(|e| format!("Remplacement interrompu: {}", e))?;

    match outcome {
        Ok(()) => {
            crate::toast::show_done(&app, done_label);
            Ok(TransformOutcome {
                text,
                replaced: true,
                message: None,
            })
        }
        Err(message) => {
            crate::toast::hide(&app);
            // Repli : au moins, l'utilisateur peut coller lui-même.
            let _ = crate::clipboard::set_clipboard(&app, text.clone());
            Ok(TransformOutcome {
                text,
                replaced: false,
                message: Some(message),
            })
        }
    }
}

#[tauri::command]
pub async fn hide_popup(app: AppHandle) -> Result<(), String> {
    popup::hide(&app);
    Ok(())
}

#[tauri::command]
pub async fn open_main_window(app: AppHandle) -> Result<(), String> {
    popup::hide(&app);
    if let Some(window) = app.get_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    Ok(())
}

#[tauri::command]
pub async fn accessibility_status() -> Result<crate::permissions::AccessibilityStatus, String> {
    Ok(crate::permissions::status())
}

#[tauri::command]
pub async fn get_history(app: AppHandle) -> Result<Vec<HistoryEntry>, String> {
    Ok(config::history(&app))
}

#[tauri::command]
pub async fn clear_history(app: AppHandle) -> Result<(), String> {
    config::clear_history(&app)
}
