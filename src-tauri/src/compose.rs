//! Réponse proposée écrite directement dans la zone de saisie de Teams ou
//! d'Outlook, plutôt que dans une popup à recopier.
//!
//! Relancer le raccourci pendant que la proposition est encore intacte dans
//! la zone de saisie l'efface et en propose une autre, différente de toutes
//! les précédentes. Si l'utilisateur l'a retouchée entre-temps, son texte
//! devient un brouillon dont la nouvelle proposition s'inspire.

use crate::config;
use lazy_static::lazy_static;
use std::sync::Mutex;
use tauri::AppHandle;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    Teams,
    Outlook,
}

impl Target {
    pub fn of(app_name: &str) -> Option<Target> {
        let app = app_name.to_ascii_lowercase();
        if app.starts_with("ms-teams") || app.starts_with("teams") {
            Some(Target::Teams)
        } else if app.starts_with("outlook") {
            Some(Target::Outlook)
        } else {
            None
        }
    }
}

/// Dernière proposition écrite, pour la reconnaître à la pression suivante.
#[derive(Clone)]
struct Proposed {
    window: isize,
    message: String,
    /// Toutes les propositions faites pour ce message, la dernière en fin.
    replies: Vec<String>,
}

lazy_static! {
    static ref LAST: Mutex<Option<Proposed>> = Mutex::new(None);
}

/// Message auquel proposer une réponse : la sélection si l'utilisateur en a
/// fait une, et elle seule, avec la conversation en contexte ; sinon le
/// dernier message reçu, lu dans l'application. Jamais de `Ctrl+A` : dans un
/// fil de discussion, il sélectionnerait toute la page.
pub fn capture_message(app: &AppHandle, app_name: &str) -> String {
    let window = crate::selection::target_window();
    let selected = match crate::inbox::focused_selection() {
        Ok(Some(text)) if !text.trim().is_empty() => text,
        // Une liste a le focus : `Ctrl+C` copierait l'élément (le courriel
        // entier dans Outlook), pas une sélection de texte.
        Ok(None) => String::new(),
        // Pas de sélection lisible par l'accessibilité, ou application qui ne
        // la publie pas : le presse-papiers tranche.
        _ => crate::selection::capture_with_mode(app, config::CAPTURE_SELECTION)
            .unwrap_or_default(),
    };
    if !selected.trim().is_empty() {
        return crate::inbox::with_context(&selected, window, app_name);
    }
    crate::inbox::last_received(window, app_name).unwrap_or_default()
}

/// Mots comparés sans tenir compte des espaces, retours à la ligne et
/// guillemets que la zone de saisie a pu réécrire.
fn comparable(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(['’', '‘'], "'")
        .to_lowercase()
}

fn same_text(a: &str, b: &str) -> bool {
    let (a, b) = (comparable(a), comparable(b));
    !a.is_empty() && a == b
}

/// La bulle envoyée est-elle, à quelques retouches près, la proposition ?
fn looks_sent(sent: &str, proposed: &str) -> bool {
    let words = |t: &str| {
        comparable(t)
            .split(' ')
            .map(str::to_string)
            .collect::<std::collections::HashSet<_>>()
    };
    let (s, p) = (words(sent), words(proposed));
    if s.is_empty() || p.is_empty() {
        return false;
    }
    let common = s.intersection(&p).count() as f64;
    common / (s.len().max(p.len()) as f64) >= 0.6
}

/// Consigne qui écarte les propositions déjà refusées.
fn different_hint(replies: &[String]) -> String {
    let mut hint = String::from(
        "L'utilisateur a rejeté les propositions ci-dessous. Propose une réponse \
         nettement différente : autre formulation, et autre approche si le contexte \
         le permet. Ne reprends pas leurs phrases.\n",
    );
    for (i, r) in replies.iter().enumerate() {
        hint.push_str(&format!("--- Proposition rejetée {} ---\n{}\n", i + 1, r.trim()));
    }
    hint
}

fn draft_hint(draft: &str) -> String {
    format!(
        "L'utilisateur a commencé à écrire ceci dans la zone de réponse. C'est son \
         brouillon, ou une consigne sur ce qu'il veut répondre : rédige la réponse \
         à partir de lui, et remplace-le.\n{}",
        draft.trim()
    )
}

/// Une proposition précédente a été envoyée telle quelle ou presque : elle
/// rejoint les réponses passées, comme une réponse copiée depuis la popup.
fn learn_if_sent(app: &AppHandle, previous: &Proposed, target: Target) {
    if target != Target::Teams {
        return;
    }
    let Some(proposed) = previous.replies.last() else {
        return;
    };
    let messages = crate::inbox::chat_messages(previous.window);
    if let Some(sent) = messages
        .iter()
        .rev()
        .take(10)
        .find(|m| m.mine && looks_sent(&m.text, proposed))
    {
        let _ = crate::reply::remember(app, previous.message.clone(), sent.text.clone());
    }
}

/// Rédige la réponse et l'écrit dans la zone de saisie de `target`.
///
/// Retourne `false` sans rien faire quand aucune zone de saisie n'est
/// trouvée (Teams ouvert sur le calendrier, par exemple) : l'appelant se
/// rabat alors sur la popup.
pub fn reply_in_place(app: &AppHandle, app_name: &str, target: Target) -> bool {
    let window = crate::selection::target_window();

    // Dans Outlook, aucune zone de saisie n'existe avant « Répondre » :
    // l'absence de brouillon n'empêche rien.
    let draft = platform::compose_text(window, target);
    if draft.is_none() && target == Target::Teams {
        return false;
    }
    let draft = draft.unwrap_or_default();

    let previous = LAST
        .lock()
        .unwrap()
        .clone()
        .filter(|p| p.window == window);
    let to_replace = previous
        .as_ref()
        .and_then(|p| p.replies.last())
        .filter(|r| platform::compose_contains(window, target, &draft, r))
        .cloned();

    let (message, hint, mut replies) = match (&previous, &to_replace) {
        (Some(p), Some(_)) => (p.message.clone(), Some(different_hint(&p.replies)), p.replies.clone()),
        _ => {
            if let Some(p) = &previous {
                learn_if_sent(app, p, target);
            }
            let message = capture_message(app, app_name);
            if message.trim().is_empty() {
                let _ = crate::popup::show_with_intent(app, String::new(), config::REPLY_ACTION.to_string());
                return true;
            }
            let hint = (!draft.trim().is_empty()).then(|| draft_hint(&draft));
            (message, hint, Vec::new())
        }
    };

    crate::toast::show_analyzing(app);
    let cfg = config::get(app);
    let provider = cfg.default_provider.clone();
    let suggestion = tauri::async_runtime::block_on(crate::reply::suggest(
        app,
        &cfg,
        &provider,
        &message,
        hint.as_deref(),
    ));
    let text = match suggestion {
        Ok(s) => s.text,
        Err(e) => {
            crate::toast::hide(app);
            crate::popup::show_error(app, e, None);
            return true;
        }
    };

    // La zone de saisie remplacée doit être celle qu'on a lue : si
    // l'utilisateur a changé de conversation pendant la rédaction, on
    // n'écrit rien dans la nouvelle.
    if crate::selection::foreground_window() != window {
        crate::toast::hide(app);
        crate::popup::show_error(
            app,
            "Vous avez changé de fenêtre pendant la rédaction : la réponse n'a pas été écrite.".to_string(),
            Some(text),
        );
        return true;
    }

    match platform::write(app, window, target, &text, to_replace.as_deref()) {
        Ok(()) => {
            crate::toast::show_done(app, "Réponse proposée");
            replies.push(text.clone());
            *LAST.lock().unwrap() = Some(Proposed {
                // Outlook peut ouvrir la réponse dans une nouvelle fenêtre.
                window: crate::selection::target_window(),
                message: message.clone(),
                replies,
            });
            let _ = config::push_history(
                app,
                config::HistoryEntry {
                    id: format!("{}", config::now_millis()),
                    original: message,
                    transformed: text,
                    action: config::REPLY_ACTION.to_string(),
                    provider,
                    timestamp: config::now_millis(),
                },
            );
        }
        Err(e) => {
            crate::toast::hide(app);
            crate::popup::show_error(app, e, Some(text));
        }
    }
    true
}

#[cfg(target_os = "windows")]
mod platform {
    use super::Target;
    use std::thread;
    use std::time::Duration;
    use tauri::AppHandle;
    use uiautomation::controls::ControlType;
    use uiautomation::patterns::{UITextPattern, UIValuePattern};
    use uiautomation::types::{Handle, TextPatternRangeEndpoint, TreeScope, UIProperty};
    use uiautomation::variants::Variant;
    use uiautomation::{UIAutomation, UIElement};

    fn automation() -> Option<UIAutomation> {
        UIAutomation::new().or_else(|_| UIAutomation::new_direct()).ok()
    }

    fn root_of(automation: &UIAutomation, window: isize) -> Option<UIElement> {
        automation.element_from_handle(Handle::from(window)).ok()
    }

    fn of_type(automation: &UIAutomation, root: &UIElement, control: ControlType) -> Vec<UIElement> {
        automation
            .create_property_condition(UIProperty::ControlType, Variant::from(control as i32), None)
            .ok()
            .and_then(|c| root.find_all(TreeScope::Descendants, &c).ok())
            .unwrap_or_default()
    }

    fn text_of(element: &UIElement) -> Option<String> {
        if let Ok(pattern) = element.get_pattern::<UITextPattern>() {
            if let Ok(text) = pattern.get_document_range().and_then(|r| r.get_text(-1)) {
                return Some(text);
            }
        }
        element.get_pattern::<UIValuePattern>().ok()?.get_value().ok()
    }

    /// Zone « Taper un message » de Teams : un `Edit` d'identifiant
    /// `new-message-<guid>`. Une conversation de canal en a une par fil
    /// ouvert ; celle qui a le focus l'emporte, sinon la dernière.
    fn teams_compose(automation: &UIAutomation, window: isize) -> Option<UIElement> {
        let root = root_of(automation, window)?;
        let edits: Vec<UIElement> = of_type(automation, &root, ControlType::Edit)
            .into_iter()
            .filter(|e| {
                e.get_automation_id()
                    .map(|id| id.starts_with("new-message-"))
                    .unwrap_or(false)
            })
            .collect();
        edits
            .iter()
            .find(|e| e.has_keyboard_focus().unwrap_or(false))
            .or_else(|| edits.last())
            .cloned()
    }

    /// Texte de la zone de saisie Teams, sans son libellé d'invite : vide,
    /// elle annonce « Taper un message ».
    fn teams_text(compose: &UIElement) -> String {
        let text = text_of(compose).unwrap_or_default();
        let placeholder = compose.get_name().unwrap_or_default();
        if text.trim() == placeholder.trim() {
            String::new()
        } else {
            text
        }
    }

    /// Une réponse en cours dans Outlook a un bouton « Envoyer » ; le volet
    /// de lecture n'en a pas.
    fn outlook_composing(automation: &UIAutomation, root: &UIElement) -> bool {
        of_type(automation, root, ControlType::Button).iter().any(|b| {
            matches!(b.get_name().as_deref(), Ok("Envoyer") | Ok("Send"))
        })
    }

    /// Corps de la réponse en cours : le plus long document de la fenêtre,
    /// celui qui contient le fil cité.
    fn outlook_body(automation: &UIAutomation, root: &UIElement) -> Option<UIElement> {
        of_type(automation, root, ControlType::Document)
            .into_iter()
            .max_by_key(|d| text_of(d).map(|t| t.len()).unwrap_or(0))
    }

    /// `None` : pas de zone de saisie (Teams hors conversation, Outlook sans
    /// réponse en cours).
    pub fn compose_text(window: isize, target: Target) -> Option<String> {
        let automation = automation()?;
        match target {
            Target::Teams => teams_compose(&automation, window).map(|c| teams_text(&c)),
            Target::Outlook => {
                let root = root_of(&automation, window)?;
                if !outlook_composing(&automation, &root) {
                    return None;
                }
                outlook_body(&automation, &root).and_then(|b| text_of(&b))
            }
        }
    }

    /// La proposition `reply` est-elle encore intacte dans la zone de saisie ?
    /// Dans Teams, elle doit en être tout le contenu ; dans Outlook, elle
    /// précède le fil cité.
    pub fn compose_contains(_window: isize, target: Target, draft: &str, reply: &str) -> bool {
        match target {
            Target::Teams => super::same_text(draft, reply),
            Target::Outlook => {
                let (latest, _) = crate::inbox::split_first_message(draft);
                super::same_text(&latest, reply)
            }
        }
    }

    fn first_and_last_lines(text: &str) -> Option<(String, String)> {
        let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let clip = |l: &str| l.chars().take(80).collect::<String>();
        let last = lines.last()?;
        // La fin de la dernière ligne, pour que la plage couvre tout.
        let tail: String = {
            let chars: Vec<char> = last.chars().collect();
            chars[chars.len().saturating_sub(80)..].iter().collect()
        };
        Some((clip(lines[0]), tail))
    }

    /// Sélectionne, dans le corps Outlook, la proposition précédente.
    fn select_in_outlook(body: &UIElement, previous: &str) -> Result<(), String> {
        let (first, last) = first_and_last_lines(previous).ok_or("Proposition vide")?;
        let pattern = body.get_pattern::<UITextPattern>().map_err(|e| e.to_string())?;
        let document = pattern.get_document_range().map_err(|e| e.to_string())?;
        let start = document.find_text(&first, false, false).map_err(|e| e.to_string())?;
        let end = document.find_text(&last, false, false).map_err(|e| e.to_string())?;
        start
            .move_endpoint_by_range(TextPatternRangeEndpoint::End, &end, TextPatternRangeEndpoint::End)
            .map_err(|e| e.to_string())?;
        start.select().map_err(|e| e.to_string())
    }

    pub fn write(
        app: &AppHandle,
        window: isize,
        target: Target,
        text: &str,
        previous: Option<&str>,
    ) -> Result<(), String> {
        let automation = automation().ok_or("Accessibilité Windows indisponible")?;
        match target {
            Target::Teams => {
                let compose = teams_compose(&automation, window)
                    .ok_or("Zone de saisie Teams introuvable")?;
                compose.set_focus().map_err(|e| e.to_string())?;
                thread::sleep(Duration::from_millis(60));
                // Dans la zone de saisie, `Ctrl+A` ne prend que son contenu :
                // la proposition précédente ou le brouillon, qu'on remplace.
                crate::selection::select_all();
                thread::sleep(Duration::from_millis(60));
                crate::selection::insert_text(app, text.to_string())
            }
            Target::Outlook => {
                let mut root = root_of(&automation, window).ok_or("Fenêtre Outlook introuvable")?;
                if !outlook_composing(&automation, &root) {
                    // « Répondre » : en ligne dans le volet de lecture, ou dans
                    // une nouvelle fenêtre selon les réglages d'Outlook.
                    crate::selection::press_ctrl('r');
                    let mut ready = false;
                    for _ in 0..40 {
                        thread::sleep(Duration::from_millis(100));
                        crate::selection::remember_target_window();
                        let current = crate::selection::target_window();
                        if let Some(r) = root_of(&automation, current) {
                            if outlook_composing(&automation, &r) {
                                root = r;
                                ready = true;
                                break;
                            }
                        }
                    }
                    if !ready {
                        return Err("Outlook n'a pas ouvert de réponse.".to_string());
                    }
                    thread::sleep(Duration::from_millis(250));
                }
                let body = outlook_body(&automation, &root).ok_or("Corps du courriel introuvable")?;
                body.set_focus().map_err(|e| e.to_string())?;
                thread::sleep(Duration::from_millis(80));
                match previous {
                    Some(previous) if select_in_outlook(&body, previous).is_ok() => {}
                    // Le point d'insertion d'une réponse neuve est en tête du
                    // corps, au-dessus du fil cité.
                    _ => crate::selection::press_ctrl_key(enigo::Key::Home),
                }
                thread::sleep(Duration::from_millis(60));
                crate::selection::insert_text(app, text.to_string())
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::Target;
    use tauri::AppHandle;

    pub fn compose_text(_window: isize, _target: Target) -> Option<String> {
        None
    }
    pub fn compose_contains(_window: isize, _target: Target, _draft: &str, _reply: &str) -> bool {
        false
    }
    pub fn write(
        _app: &AppHandle,
        _window: isize,
        _target: Target,
        _text: &str,
        _previous: Option<&str>,
    ) -> Result<(), String> {
        Err("Écriture directe propre à Windows".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{looks_sent, same_text};

    #[test]
    fn proposition_intacte_malgre_la_mise_en_forme() {
        assert!(same_text("Salut Carl,\r\n\r\nC’est réglé.", "salut carl,\nC'est réglé."));
        assert!(!same_text("Salut Carl, c'est réglé. Merci", "Salut Carl, c'est réglé."));
        assert!(!same_text("", ""));
    }

    #[test]
    fn proposition_envoyee_avec_retouches() {
        let proposed = "Salut Carl, tu changes le statut à « en cours » puis tu complètes avec un commentaire.";
        assert!(looks_sent("Salut Carl, tu changes le statut à « en cours » puis tu complètes avec un petit commentaire.", proposed));
        assert!(!looks_sent("ok merci", proposed));
    }
}
