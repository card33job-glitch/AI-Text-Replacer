//! Autorisations système requises pour piloter une autre application.
//!
//! macOS interdit par défaut à un programme d'envoyer des frappes clavier ou
//! de lire le contenu d'une autre application. Sans l'autorisation
//! « Accessibilité », les raccourcis s'enregistrent, se déclenchent, et ne
//! produisent rien — un échec entièrement silencieux, d'où ce contrôle
//! explicite exposé à l'interface.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessibilityStatus {
    /// Faux uniquement sur macOS tant que l'autorisation n'est pas accordée.
    pub granted: bool,
    /// Vrai sur les plateformes qui exigent une autorisation (macOS seul).
    pub required: bool,
    /// Lien vers le panneau de réglages concerné, à ouvrir pour l'accorder.
    pub settings_url: Option<String>,
}

#[cfg(target_os = "macos")]
pub fn status() -> AccessibilityStatus {
    // `Boolean` côté C est un `unsigned char` : on le reçoit en u8 plutôt
    // qu'en bool, dont le domaine de valeurs est plus étroit.
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    AccessibilityStatus {
        granted: unsafe { AXIsProcessTrusted() != 0 },
        required: true,
        settings_url: Some(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
                .to_string(),
        ),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> AccessibilityStatus {
    AccessibilityStatus {
        granted: true,
        required: false,
        settings_url: None,
    }
}
