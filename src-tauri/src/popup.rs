//! Petite fenêtre sans bordure affichée près du curseur : l'équivalent le plus
//! proche d'un menu contextuel qu'une application tierce puisse offrir, puisque
//! Windows ne permet pas d'ajouter des entrées au menu clic droit d'une autre
//! application (Teams dessine le sien lui-même, en HTML).

use lazy_static::lazy_static;
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize};

pub const POPUP_LABEL: &str = "popup";
const POPUP_WIDTH: f64 = 360.0;
const POPUP_HEIGHT: f64 = 320.0;
const MARGIN: i32 = 12;

lazy_static! {
    /// Texte capturé en attente de transformation. Conservé ici pour que la
    /// popup puisse le redemander si elle n'était pas encore prête à recevoir
    /// l'événement (premier affichage après le démarrage).
    static ref PENDING: Mutex<String> = Mutex::new(String::new());
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub text: String,
    pub default_provider: String,
    pub preview_before_replace: bool,
    pub target_language: String,
}

pub fn pending_selection(app: &AppHandle) -> Selection {
    let cfg = crate::config::get(app);
    Selection {
        text: PENDING.lock().unwrap().clone(),
        default_provider: cfg.default_provider,
        preview_before_replace: cfg.preview_before_replace,
        target_language: cfg.target_language,
    }
}

pub fn show(app: &AppHandle, text: String) -> Result<(), String> {
    let window = app
        .get_window(POPUP_LABEL)
        .ok_or_else(|| "Fenêtre popup introuvable".to_string())?;

    *PENDING.lock().unwrap() = text;
    let selection = pending_selection(app);

    window
        .set_size(LogicalSize::new(POPUP_WIDTH, POPUP_HEIGHT))
        .map_err(|e| e.to_string())?;

    if let Some(position) = anchor_position(&window) {
        let _ = window.set_position(position);
    }

    // Émettre avant d'afficher évite un scintillement du contenu précédent.
    let _ = window.emit("selection-captured", selection);
    window.show().map_err(|e| e.to_string())?;
    let _ = window.set_always_on_top(true);
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformError {
    pub message: String,
    /// Résultat produit malgré l'échec du collage, s'il y en a un : mieux vaut
    /// proposer de le copier que de le perdre.
    pub result: Option<String>,
}

/// Affiche la popup en mode erreur. Utilisé par les raccourcis directs, qui
/// n'ont sinon aucune surface pour signaler un problème.
pub fn show_error(app: &AppHandle, message: String, result: Option<String>) {
    let window = match app.get_window(POPUP_LABEL) {
        Some(w) => w,
        None => {
            eprintln!("Popup introuvable, erreur perdue: {}", message);
            return;
        }
    };

    let _ = window.set_size(LogicalSize::new(POPUP_WIDTH, POPUP_HEIGHT));
    if let Some(position) = anchor_position(&window) {
        let _ = window.set_position(position);
    }
    let _ = window.emit("transform-error", TransformError { message, result });
    let _ = window.show();
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_window(POPUP_LABEL) {
        let _ = window.hide();
    }
}

/// Place la popup juste sous le curseur, sans déborder de l'écran courant.
fn anchor_position(window: &tauri::Window) -> Option<PhysicalPosition<i32>> {
    let (cursor_x, cursor_y) = crate::selection::cursor_position()?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let size = PhysicalSize::new(
        (POPUP_WIDTH * scale) as i32,
        (POPUP_HEIGHT * scale) as i32,
    );

    let monitors = window.available_monitors().unwrap_or_default();
    let monitor = monitors
        .iter()
        .find(|m| {
            let p = m.position();
            let s = m.size();
            cursor_x >= p.x
                && cursor_x < p.x + s.width as i32
                && cursor_y >= p.y
                && cursor_y < p.y + s.height as i32
        })
        .or_else(|| monitors.first());

    let (min_x, min_y, max_x, max_y) = match monitor {
        Some(m) => {
            let p = m.position();
            let s = m.size();
            (
                p.x + MARGIN,
                p.y + MARGIN,
                p.x + s.width as i32 - size.width - MARGIN,
                p.y + s.height as i32 - size.height - MARGIN,
            )
        }
        None => (0, 0, i32::MAX, i32::MAX),
    };

    let x = (cursor_x + MARGIN).clamp(min_x, max_x.max(min_x));
    let y = (cursor_y + MARGIN).clamp(min_y, max_y.max(min_y));
    Some(PhysicalPosition::new(x, y))
}
