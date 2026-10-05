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

/// Demande l'autorisation à macOS si elle manque.
///
/// `AXIsProcessTrusted` se contente de vérifier : sans cet appel, macOS
/// n'affiche jamais sa boîte de dialogue et l'application n'apparaît même pas
/// dans la liste « Accessibilité » des Réglages. Avec l'option `Prompt`, elle
/// y est ajoutée (désactivée) et la boîte propose d'ouvrir les Réglages.
#[cfg(target_os = "macos")]
pub fn request() {
    use std::ffi::c_void;
    type CFTypeRef = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: CFTypeRef;
        fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFBooleanTrue: CFTypeRef;
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
        fn CFDictionaryCreate(
            allocator: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            count: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> CFTypeRef;
        fn CFRelease(cf: CFTypeRef);
    }

    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );
        if options.is_null() {
            return;
        }
        AXIsProcessTrustedWithOptions(options);
        CFRelease(options);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn request() {}

#[cfg(not(target_os = "macos"))]
pub fn status() -> AccessibilityStatus {
    AccessibilityStatus {
        granted: true,
        required: false,
        settings_url: None,
    }
}
