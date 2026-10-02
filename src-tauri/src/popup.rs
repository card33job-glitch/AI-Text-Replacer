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
/// Une réponse proposée se relit et se retouche : il lui faut de la place.
const REPLY_WIDTH: f64 = 480.0;
const REPLY_HEIGHT: f64 = 480.0;
const MARGIN: i32 = 12;

lazy_static! {
    /// Texte capturé en attente de transformation, et action demandée d'office
    /// (vide = laisser choisir). Conservés ici pour que la popup puisse les
    /// redemander si elle n'était pas encore prête à recevoir l'événement
    /// (premier affichage après le démarrage).
    static ref PENDING: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub text: String,
    /// Action à lancer dès l'ouverture (« reply »), ou vide pour le menu.
    pub intent: String,
    pub default_provider: String,
    pub preview_before_replace: bool,
    pub target_language: String,
}

pub fn pending_selection(app: &AppHandle) -> Selection {
    let cfg = crate::config::get(app);
    let (text, intent) = PENDING.lock().unwrap().clone();
    Selection {
        text,
        intent,
        default_provider: cfg.default_provider,
        preview_before_replace: cfg.preview_before_replace,
        target_language: cfg.target_language,
    }
}

pub fn show(app: &AppHandle, text: String) -> Result<(), String> {
    show_with_intent(app, text, String::new())
}

pub fn show_with_intent(app: &AppHandle, text: String, intent: String) -> Result<(), String> {
    let window = app
        .get_window(POPUP_LABEL)
        .ok_or_else(|| "Fenêtre popup introuvable".to_string())?;

    let (width, height) = if intent == crate::config::REPLY_ACTION {
        (REPLY_WIDTH, REPLY_HEIGHT)
    } else {
        (POPUP_WIDTH, POPUP_HEIGHT)
    };
    *PENDING.lock().unwrap() = (text, intent);
    let selection = pending_selection(app);

    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;

    if let Some(position) = anchor_position(&window, width, height) {
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
    if let Some(position) = anchor_position(&window, POPUP_WIDTH, POPUP_HEIGHT) {
        let _ = window.set_position(position);
    }
    let _ = window.emit("transform-error", TransformError { message, result });
    let _ = window.show();
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();
}

/// Agrandit la popup déjà ouverte au format « réponse », quand l'action est
/// choisie depuis le menu plutôt que par son raccourci.
pub fn expand_for_reply(app: &AppHandle) {
    if let Some(window) = app.get_window(POPUP_LABEL) {
        let _ = window.set_size(LogicalSize::new(REPLY_WIDTH, REPLY_HEIGHT));
        // Le coin haut gauche reste en place : on ne fait que vérifier que
        // l'agrandissement ne déborde pas de l'écran.
        if let (Ok(position), Some(monitor)) = (window.outer_position(), window.current_monitor().ok().flatten()) {
            let scale = window.scale_factor().unwrap_or(1.0);
            let (w, h) = ((REPLY_WIDTH * scale) as i32, (REPLY_HEIGHT * scale) as i32);
            let (mp, ms) = (monitor.position(), monitor.size());
            let x = position.x.min(mp.x + ms.width as i32 - w - MARGIN).max(mp.x + MARGIN);
            let y = position.y.min(mp.y + ms.height as i32 - h - MARGIN).max(mp.y + MARGIN);
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_window(POPUP_LABEL) {
        let _ = window.hide();
    }
}

/// Place la popup juste sous le curseur, sans déborder de l'écran courant.
fn anchor_position(
    window: &tauri::Window,
    width: f64,
    height: f64,
) -> Option<PhysicalPosition<i32>> {
    let (cursor_x, cursor_y) = crate::selection::cursor_position()?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let size = PhysicalSize::new((width * scale) as i32, (height * scale) as i32);

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
