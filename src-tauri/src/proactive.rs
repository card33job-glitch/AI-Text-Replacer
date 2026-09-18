//! Contrôle grammatical spontané, sans raccourci.
//!
//! Dans les applications listées par l'utilisateur, une pause dans la frappe
//! déclenche une lecture du champ de saisie et, si le modèle y trouve des
//! fautes, un remplacement immédiat.
//!
//! # Ce que ce module ne peut pas faire
//!
//! Aucune application ne peut lire le champ de saisie d'une autre. La lecture
//! passe donc, comme partout ailleurs ici, par un `Ctrl+A` suivi d'un `Ctrl+C`.
//! Deux conséquences dont tout le reste découle :
//!
//! - pendant un très court instant, le champ de l'utilisateur est **entièrement
//!   sélectionné**. Une frappe à cet instant précis effacerait tout. C'est la
//!   raison d'être du délai d'inactivité : on ne lit que quelqu'un qui s'est
//!   arrêté de taper ;
//! - une lecture qui n'aboutit à aucun remplacement doit **replier la
//!   sélection**, sinon c'est la frappe suivante de l'utilisateur qui la
//!   remplacerait. Chaque sortie de `run_check` passe par `collapse_selection`.
//!
//! Le remplacement est abandonné, la sélection repliée, si entre la lecture et
//! le collage l'utilisateur s'est remis à taper ou a changé d'application :
//! coller à ce moment-là écraserait ce qu'il vient d'écrire, ou déposerait son
//! texte dans une autre fenêtre.

use crate::config::{self, AppConfig, HistoryEntry};
use lazy_static::lazy_static;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// Cadence d'observation. Assez fin pour que le délai choisi soit tenu à une
/// demi-seconde près, assez lâche pour ne rien coûter : deux appels système.
const POLL_MS: u64 = 700;

/// En deçà, on considère que l'utilisateur est encore en train d'agir. C'est ce
/// qui réarme le contrôle entre deux pauses.
const ACTIVE_MS: u64 = 1_000;

/// Tolérance sur la détection « il s'est remis à taper ». Les mesures
/// d'inactivité des deux systèmes ne sont pas au millimètre, et notre propre
/// simulation de frappes compte comme une entrée.
const INPUT_TOLERANCE_MS: u64 = 250;

lazy_static! {
    /// Dernière application au premier plan qui ne soit pas la nôtre.
    /// Les Paramètres s'en servent pour proposer un nom à ajouter : une fois la
    /// fenêtre des Paramètres au premier plan, l'application « active » serait
    /// la nôtre, donc inutile à afficher.
    static ref LAST_APP: Mutex<Option<String>> = Mutex::new(None);
}

pub fn last_foreground_app() -> Option<String> {
    LAST_APP.lock().unwrap().clone()
}

/// Lance l'observateur. Un seul appel, au démarrage.
///
/// La boucle tourne même quand le mode est désactivé : elle entretient alors
/// `LAST_APP`, ce qui permet aux Paramètres de proposer un nom d'application
/// sans rien demander à l'utilisateur.
pub fn start(app: AppHandle) {
    thread::spawn(move || {
        // Texte du dernier contrôle : inutile de rappeler le modèle sur un
        // contenu qu'il vient de voir.
        let mut last_seen = String::new();
        // Faux tant que l'utilisateur n'a rien fait depuis le dernier contrôle.
        // Sans cela, une pause prolongée relancerait un contrôle à chaque tour.
        let mut armed = false;
        let mut watched_before = false;

        loop {
            thread::sleep(Duration::from_millis(POLL_MS));

            let current = crate::selection::foreground_app();
            if let Some(name) = current.as_ref() {
                if !is_self(name) {
                    *LAST_APP.lock().unwrap() = Some(name.clone());
                }
            }

            let cfg = config::get(&app);
            if !cfg.proactive.enabled || cfg.proactive.apps.is_empty() {
                continue;
            }

            let watched = current.as_deref().map(|n| is_watched(&cfg, n)).unwrap_or(false);
            // Changer d'application remet le compteur à zéro : le champ observé
            // n'est plus le même, et son contenu n'a rien à voir avec l'ancien.
            if watched != watched_before {
                watched_before = watched;
                last_seen.clear();
                armed = false;
            }
            if !watched {
                continue;
            }

            let idle = crate::selection::idle_millis();
            if idle < ACTIVE_MS {
                armed = true;
                continue;
            }
            if !armed || idle < cfg.proactive.idle_seconds.saturating_mul(1_000) {
                continue;
            }

            // Une seule tentative par pause, quoi qu'il advienne ensuite.
            armed = false;

            // Un raccourci manuel est en cours : ses frappes simulées et les
            // nôtres se mélangeraient.
            if !crate::shortcuts::try_begin() {
                continue;
            }
            let checked = run_check(&app, &cfg, &last_seen);
            crate::shortcuts::end();

            if let Some(text) = checked {
                last_seen = text;
            }
        }
    });
}

/// Un tour complet de contrôle. Retourne le texte à retenir comme « déjà vu »,
/// ou `None` s'il n'y a rien à retenir.
///
/// Chaque retour anticipé replie la sélection : voir l'avertissement en tête de
/// module.
fn run_check(app: &AppHandle, cfg: &AppConfig, last_seen: &str) -> Option<String> {
    crate::selection::remember_target_window();
    let target = crate::selection::target_window();

    let text = crate::selection::capture_with_mode(app, config::CAPTURE_FIELD).ok()?;
    // Repère pris *après* nos propres frappes simulées, qui comptent comme une
    // entrée utilisateur pour le système.
    let since_capture = Instant::now();

    let trimmed = text.trim();
    if trimmed.chars().count() < cfg.proactive.min_chars || trimmed == last_seen.trim() {
        crate::selection::collapse_selection();
        // Un champ trop court reste retenu : tant qu'il ne change pas, inutile
        // d'y revenir.
        return Some(text);
    }

    crate::toast::show_analyzing(app);
    let provider = cfg.default_provider.clone();
    let corrected =
        match tauri::async_runtime::block_on(crate::ai::transform(cfg, &provider, "grammar", &text))
        {
            Ok(corrected) => corrected,
            Err(e) => {
                // Silence : l'utilisateur n'a rien demandé, une popup d'erreur
                // au milieu de sa frappe serait une intrusion. La trace suffit.
                eprintln!("Contrôle spontané: {}", e);
                crate::toast::hide(app);
                crate::selection::collapse_selection();
                return Some(text);
            }
        };

    if corrected.trim() == trimmed {
        // Rien à corriger : ne pas coller un texte identique, qui ferait
        // clignoter le champ et le marquerait modifié pour rien.
        crate::toast::hide(app);
        crate::selection::collapse_selection();
        return Some(text);
    }

    if user_moved_on(target, since_capture) {
        crate::toast::hide(app);
        crate::selection::collapse_selection();
        return Some(text);
    }

    let _ = config::push_history(
        app,
        HistoryEntry {
            id: format!("{}", config::now_millis()),
            original: text,
            transformed: corrected.clone(),
            action: "grammar".to_string(),
            provider,
            timestamp: config::now_millis(),
        },
    );

    match crate::selection::replace_selection(app, corrected.clone()) {
        Ok(()) => {
            crate::toast::show_done(app, "Corrigé");
            // C'est le texte corrigé qui est maintenant dans le champ : le
            // retenir évite de le soumettre à nouveau à la pause suivante.
            Some(corrected)
        }
        Err(e) => {
            eprintln!("Contrôle spontané: {}", e);
            crate::toast::hide(app);
            None
        }
    }
}

/// Vrai si coller maintenant écraserait autre chose que ce qu'on a lu.
///
/// Deux cas : l'utilisateur s'est remis à taper — sa frappe a déjà remplacé la
/// sélection, et notre collage effacerait ce qu'il vient d'écrire —, ou il est
/// passé à une autre fenêtre, que `replace_selection` quitterait de force pour
/// revenir coller ici.
fn user_moved_on(target: isize, since_capture: Instant) -> bool {
    if crate::selection::foreground_window() != target {
        return true;
    }
    let elapsed = since_capture.elapsed().as_millis() as u64;
    // Sans nouvelle entrée, l'inactivité mesurée a grandi d'autant que le temps
    // écoulé depuis notre repère. Nettement moins : quelqu'un a tapé entre-temps.
    crate::selection::idle_millis() + INPUT_TOLERANCE_MS < elapsed
}

/// Comparaison tolérante : l'utilisateur peut écrire « Teams », « teams.exe »
/// ou « ms-teams.exe » sans que ça change son intention.
fn is_watched(cfg: &AppConfig, app_name: &str) -> bool {
    let actual = normalize(app_name);
    cfg.proactive
        .apps
        .iter()
        .map(|entry| normalize(entry))
        .any(|entry| !entry.is_empty() && (entry == actual || actual.contains(&entry)))
}

fn normalize(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower
        .strip_suffix(".exe")
        .or_else(|| lower.strip_suffix(".app"))
        .unwrap_or(&lower)
        .to_string()
}

/// Notre propre fenêtre n'a pas à être proposée comme application à surveiller.
fn is_self(app_name: &str) -> bool {
    let normalized = normalize(app_name);
    normalized == "ai-text-replacer" || normalized == "ai text replacer"
}
