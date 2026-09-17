//! Témoin discret posé près du point d'insertion : « Analyse… » pendant
//! l'appel au modèle, puis « Corrigé ✓ » une fois le texte remplacé, et il
//! s'efface au bout d'une seconde et demie.
//!
//! Sa contrainte principale est de **ne jamais prendre le focus**. Il est
//! affiché pendant que l'utilisateur continue à taper : une fenêtre qui
//! s'active lui volerait ses frappes. D'où le style étendu `WS_EX_NOACTIVATE`,
//! posé sur le handle natif avant le premier affichage.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Window};

pub const TOAST_LABEL: &str = "toast";
const TOAST_WIDTH: f64 = 168.0;
const TOAST_HEIGHT: f64 = 34.0;
const VISIBLE_MS: u64 = 1400;
/// Filet de sécurité : si un appel au modèle n'aboutit jamais, le témoin
/// « Analyse… » ne doit pas rester à l'écran indéfiniment.
const STALE_MS: u64 = 45_000;
const OFFSET: i32 = 14;

/// Numéro du témoin en cours. Deux remplacements rapprochés ne doivent pas
/// laisser le second s'éteindre à l'heure du premier.
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToastState {
    /// « analyzing » ou « done ».
    phase: &'static str,
    label: String,
}

/// Libellé affiché une fois la transformation appliquée.
pub fn done_label(action: &str) -> &'static str {
    match action {
        "grammar" => "Corrigé",
        "rephrase" => "Reformulé",
        "professional" => "Professionnel",
        "concise" => "Raccourci",
        "translate" => "Traduit",
        "summarize" => "Résumé",
        "prompt" => "Rédigé",
        _ => "Remplacé",
    }
}

/// Affiche « Analyse… » et laisse le témoin ouvert.
pub fn show_analyzing(app: &AppHandle) {
    let generation = emit(
        app,
        ToastState {
            phase: "analyzing",
            label: "Analyse".to_string(),
        },
        true,
    );
    schedule_hide(app, generation, STALE_MS);
}

/// Bascule le témoin en « Corrigé ✓ » et programme son extinction.
pub fn show_done(app: &AppHandle, label: &str) {
    let generation = emit(
        app,
        ToastState {
            phase: "done",
            label: label.to_string(),
        },
        false,
    );
    schedule_hide(app, generation, VISIBLE_MS);
}

/// Referme le témoin immédiatement (erreur : c'est la popup qui prend le relais).
pub fn hide(app: &AppHandle) {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(window) = app.get_window(TOAST_LABEL) {
        let _ = window.hide();
    }
}

/// Envoie l'état au témoin et retourne le numéro de génération associé.
///
/// `reposition` n'est vrai qu'à l'ouverture : une fois le témoin posé, il ne
/// doit plus sauter parce que la souris a bougé entre-temps.
fn emit(app: &AppHandle, state: ToastState, reposition: bool) -> u64 {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let window = match app.get_window(TOAST_LABEL) {
        Some(w) => w,
        None => return generation,
    };

    make_non_activating(&window);
    let _ = window.emit("toast-update", state);

    if reposition {
        let _ = window.set_size(LogicalSize::new(TOAST_WIDTH, TOAST_HEIGHT));
        if let Some(position) = anchor_position(&window) {
            let _ = window.set_position(position);
        }
    }

    platform_show(&window);
    let _ = window.set_always_on_top(true);
    generation
}

#[cfg(target_os = "macos")]
fn platform_show(window: &Window) {
    use cocoa::base::id;
    use objc::{msg_send, sel, sel_impl};

    // `show()` de Tauri appelle `makeKeyAndOrderFront:`, qui donnerait le focus
    // au témoin — exactement ce qu'il ne doit jamais faire.
    // `orderFrontRegardless` l'affiche sans le rendre fenêtre clé.
    match window.ns_window() {
        Ok(handle) => unsafe {
            let ns_window = handle as id;
            let _: () = msg_send![ns_window, orderFrontRegardless];
        },
        Err(_) => {
            let _ = window.show();
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn platform_show(window: &Window) {
    let _ = window.show();
}

fn schedule_hide(app: &AppHandle, generation: u64, delay_ms: u64) {
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(delay_ms));
        // Un état plus récent est arrivé entre-temps : c'est lui qui décidera
        // de sa propre extinction.
        if GENERATION.load(Ordering::SeqCst) == generation {
            if let Some(window) = app.get_window(TOAST_LABEL) {
                let _ = window.hide();
            }
        }
    });
}

#[cfg(target_os = "windows")]
fn make_non_activating(window: &Window) {
    use winapi::shared::windef::HWND;
    use winapi::um::winuser::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    if let Ok(handle) = window.hwnd() {
        unsafe {
            let hwnd = handle.0 as HWND;
            let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let wanted = current | (WS_EX_NOACTIVATE as isize) | (WS_EX_TOOLWINDOW as isize);
            if current != wanted {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn make_non_activating(window: &Window) {
    use cocoa::base::{id, YES};
    use objc::{msg_send, sel, sel_impl};

    if let Ok(handle) = window.ns_window() {
        unsafe {
            let ns_window = handle as id;
            // NSStatusWindowLevel : au-dessus des fenêtres ordinaires.
            let _: () = msg_send![ns_window, setLevel: 25i64];
            // Purement informatif : le témoin laisse passer les clics.
            let _: () = msg_send![ns_window, setIgnoresMouseEvents: YES];
            // Visible sur tous les bureaux, absent du cycle Cmd+Tab.
            // NSWindowCollectionBehaviorCanJoinAllSpaces | Transient
            let behavior: u64 = (1 << 0) | (1 << 3);
            let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn make_non_activating(_window: &Window) {}

/// Pose le témoin à côté du point d'insertion, ou à défaut du curseur souris.
fn anchor_position(window: &Window) -> Option<PhysicalPosition<i32>> {
    let target = crate::selection::target_window();
    let (x, y) =
        crate::selection::caret_position(target).or_else(crate::selection::cursor_position)?;

    let scale = window.scale_factor().unwrap_or(1.0);
    let size = PhysicalSize::new((TOAST_WIDTH * scale) as i32, (TOAST_HEIGHT * scale) as i32);

    let monitors = window.available_monitors().unwrap_or_default();
    let monitor = monitors
        .iter()
        .find(|m| {
            let p = m.position();
            let s = m.size();
            x >= p.x && x < p.x + s.width as i32 && y >= p.y && y < p.y + s.height as i32
        })
        .or_else(|| monitors.first());

    let (min_x, min_y, max_x, max_y) = match monitor {
        Some(m) => {
            let p = m.position();
            let s = m.size();
            (
                p.x,
                p.y,
                p.x + s.width as i32 - size.width,
                p.y + s.height as i32 - size.height,
            )
        }
        None => (0, 0, i32::MAX, i32::MAX),
    };

    Some(PhysicalPosition::new(
        (x + OFFSET).clamp(min_x, max_x.max(min_x)),
        (y + OFFSET).clamp(min_y, max_y.max(min_y)),
    ))
}
