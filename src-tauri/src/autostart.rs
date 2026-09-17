//! Démarrage automatique à l'ouverture de session.
//!
//! Windows passe par la clé `Run` de l'utilisateur courant, macOS par un
//! *LaunchAgent* dans `~/Library/LaunchAgents`. Dans les deux cas l'entrée est
//! posée pour l'utilisateur seul : aucune élévation de privilèges n'est
//! nécessaire, et rien n'est écrit pour les autres comptes de la machine.

use std::path::PathBuf;

/// Argument ajouté à la commande enregistrée. Il ne sert qu'à reconnaître un
/// lancement automatique, pour démarrer dans la zone de notification plutôt
/// que d'ouvrir une fenêtre au visage de l'utilisateur à chaque ouverture de
/// session.
const AUTOSTART_FLAG: &str = "--autostart";

/// Vrai quand ce processus a été lancé par le mécanisme de démarrage auto.
pub fn launched_at_login() -> bool {
    std::env::args().any(|arg| arg == AUTOSTART_FLAG)
}

fn exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("Chemin de l'exécutable introuvable: {}", e))
}

/// Aligne l'état du système sur la configuration.
///
/// Réécrit l'entrée quand elle pointe ailleurs : c'est ce qui rattrape une mise
/// à jour ou un déplacement de l'application, sans quoi la session ouvrirait
/// une ancienne copie — ou rien du tout.
pub fn sync(enabled: bool) -> Result<(), String> {
    match (enabled, imp::current_entry()?) {
        (true, Some(entry)) if entry == imp::desired_entry()? => Ok(()),
        (true, _) => imp::enable(),
        (false, Some(_)) => imp::disable(),
        (false, None) => Ok(()),
    }
}

/// État réel côté système, indépendamment de ce que dit la configuration.
pub fn is_enabled() -> bool {
    matches!(imp::current_entry(), Ok(Some(_)))
}

#[cfg(target_os = "windows")]
mod imp {
    use super::{exe_path, AUTOSTART_FLAG};
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// Nom de la valeur dans la clé `Run` : c'est aussi ce que le Gestionnaire
    /// des tâches affiche dans son onglet « Démarrage ».
    const VALUE_NAME: &str = "AI Text Replacer";

    /// La commande que Windows exécutera. Le chemin est entre guillemets :
    /// « C:\Program Files\… » contient des espaces.
    pub fn desired_entry() -> Result<String, String> {
        Ok(format!(
            "\"{}\" {}",
            exe_path()?.display(),
            AUTOSTART_FLAG
        ))
    }

    pub fn current_entry() -> Result<Option<String>, String> {
        let run = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_READ)
            .map_err(|e| format!("Lecture du registre: {}", e))?;
        match run.get_value::<String, _>(VALUE_NAME) {
            Ok(value) => Ok(Some(value)),
            // Absente : c'est l'état « désactivé », pas une erreur.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("Lecture du registre: {}", e)),
        }
    }

    pub fn enable() -> Result<(), String> {
        let entry = desired_entry()?;
        let run = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
            .map_err(|e| format!("Ouverture du registre: {}", e))?;
        run.set_value(VALUE_NAME, &entry)
            .map_err(|e| format!("Écriture du registre: {}", e))
    }

    pub fn disable() -> Result<(), String> {
        let run = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
            .map_err(|e| format!("Ouverture du registre: {}", e))?;
        match run.delete_value(VALUE_NAME) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Suppression dans le registre: {}", e)),
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{exe_path, AUTOSTART_FLAG};
    use std::path::PathBuf;

    /// Même identifiant que le bundle (`tauri.conf.json`), pour que l'agent soit
    /// reconnaissable dans `launchctl list`.
    const LABEL: &str = "com.aitextreplacer.app";

    fn plist_path() -> Result<PathBuf, String> {
        let home = std::env::var("HOME")
            .map_err(|_| "Dossier personnel introuvable (HOME non défini)".to_string())?;
        Ok(PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("{}.plist", LABEL)))
    }

    /// `&`, `<` et `>` sont significatifs en XML et un chemin peut en contenir.
    fn escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    pub fn desired_entry() -> Result<String, String> {
        Ok(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>{flag}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>LimitLoadToSessionType</key>
    <string>Aqua</string>
</dict>
</plist>
"#,
            label = LABEL,
            exe = escape(&exe_path()?.display().to_string()),
            flag = AUTOSTART_FLAG,
        ))
    }

    pub fn current_entry() -> Result<Option<String>, String> {
        let path = plist_path()?;
        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("Lecture de {}: {}", path.display(), e)),
        }
    }

    pub fn enable() -> Result<(), String> {
        let path = plist_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Création de {}: {}", parent.display(), e))?;
        }
        // Décharger d'abord : `launchctl` refuse de charger un agent déjà connu,
        // et le fichier vient peut-être de changer de chemin d'exécutable.
        let _ = launchctl("unload", &path);
        std::fs::write(&path, desired_entry()?)
            .map_err(|e| format!("Écriture de {}: {}", path.display(), e))?;
        // Sans ce chargement, l'agent ne prendrait effet qu'à la prochaine
        // ouverture de session.
        launchctl("load", &path)
    }

    pub fn disable() -> Result<(), String> {
        let path = plist_path()?;
        let _ = launchctl("unload", &path);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Suppression de {}: {}", path.display(), e)),
        }
    }

    fn launchctl(action: &str, path: &PathBuf) -> Result<(), String> {
        let output = std::process::Command::new("launchctl")
            .arg(action)
            .arg("-w")
            .arg(path)
            .output()
            .map_err(|e| format!("launchctl {}: {}", action, e))?;
        if output.status.success() {
            return Ok(());
        }
        Err(format!(
            "launchctl {}: {}",
            action,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Les autres systèmes compilent, mais sans démarrage automatique : mieux vaut
/// un message clair qu'une case à cocher qui ne fait rien.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod imp {
    pub fn desired_entry() -> Result<String, String> {
        Err(unsupported())
    }

    pub fn current_entry() -> Result<Option<String>, String> {
        Ok(None)
    }

    pub fn enable() -> Result<(), String> {
        Err(unsupported())
    }

    pub fn disable() -> Result<(), String> {
        Ok(())
    }

    fn unsupported() -> String {
        "Le démarrage automatique n'est pris en charge que sur Windows et macOS.".to_string()
    }
}
