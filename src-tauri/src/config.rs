use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;

pub const PROVIDERS: [&str; 4] = ["openai", "claude", "groq", "local"];

pub fn default_model(provider: &str) -> &'static str {
    match provider {
        "openai" => "gpt-4o-mini",
        "claude" => "claude-sonnet-5",
        // Le catalogue Groq tourne vite : les modèles Llama ont été retirés.
        // `GET /openai/v1/models` donne la liste à jour pour une clé donnée.
        "groq" => "openai/gpt-oss-120b",
        "local" => "llama3.1",
        _ => "",
    }
}

pub fn default_endpoint(provider: &str) -> &'static str {
    match provider {
        "openai" => "https://api.openai.com/v1/chat/completions",
        "claude" => "https://api.anthropic.com/v1/messages",
        "groq" => "https://api.groq.com/openai/v1/chat/completions",
        // Ollama / LM Studio exposent une API compatible OpenAI
        "local" => "http://localhost:11434/v1/chat/completions",
        _ => "",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub endpoint: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        ProviderConfig {
            api_key: String::new(),
            model: String::new(),
            endpoint: String::new(),
        }
    }
}

/// Une combinaison de touches et ce qu'elle déclenche.
///
/// `action` vaut soit `"menu"` (ouvre la popup et laisse choisir), soit l'id
/// d'une action de transformation appliquée directement, sans aucune UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBinding {
    pub accelerator: String,
    pub action: String,
}

pub const MENU_ACTION: &str = "menu";

/// Modificateur par défaut : un utilisateur macOS attend Cmd là où un
/// utilisateur Windows attend Ctrl.
#[cfg(target_os = "macos")]
const DEFAULT_MODIFIERS: &str = "Cmd+Shift+";
#[cfg(not(target_os = "macos"))]
const DEFAULT_MODIFIERS: &str = "Ctrl+Shift+";

pub fn default_shortcuts() -> Vec<ShortcutBinding> {
    let binding = |key: &str, action: &str| ShortcutBinding {
        accelerator: format!("{}{}", DEFAULT_MODIFIERS, key),
        action: action.to_string(),
    };
    vec![
        binding("T", MENU_ACTION),
        binding("G", "grammar"),
        binding("R", "rephrase"),
        binding("D", "prompt"),
    ]
}

fn default_provider() -> String {
    "claude".to_string()
}

fn default_language() -> String {
    "anglais".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// Raccourcis globaux, au format accélérateur Tauri ("Ctrl+Shift+T").
    #[serde(default)]
    pub shortcuts: Vec<ShortcutBinding>,
    /// Ancien champ « un seul raccourci ». Conservé en lecture seule pour
    /// migrer les configurations écrites avant l'ajout des raccourcis directs.
    #[serde(default, rename = "shortcut", skip_serializing)]
    pub legacy_shortcut: Option<String>,
    /// Fournisseur utilisé par défaut par la popup.
    #[serde(default = "default_provider")]
    pub default_provider: String,
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    /// Si vrai, la popup affiche le résultat et attend une validation
    /// avant de remplacer le texte dans l'application d'origine.
    #[serde(default)]
    pub preview_before_replace: bool,
    #[serde(default = "default_language")]
    pub target_language: String,
    /// Consignes ajoutées à chaque prompt (ex: "vouvoie toujours").
    #[serde(default)]
    pub custom_instructions: String,
    #[serde(default)]
    pub start_minimized: bool,
    /// Lance l'application à l'ouverture de session (clé `Run` sous Windows,
    /// LaunchAgent sous macOS). Voir `autostart`.
    #[serde(default)]
    pub start_at_login: bool,
    /// Ce que le raccourci capture : voir les constantes `CAPTURE_*`.
    /// Vide à la lecture d'une config antérieure, `migrate` s'en charge.
    #[serde(default)]
    pub capture_mode: String,
    /// Ancien champ booléen, conservé en lecture seule pour la migration.
    #[serde(default, rename = "selectAllIfEmpty", skip_serializing)]
    pub legacy_select_all: Option<bool>,
}

/// Toujours tout le champ de saisie : un seul Ctrl+A puis Ctrl+C.
/// C'est le seul mode qui n'envoie jamais de copie « à vide », donc le seul
/// qui ne déclenche jamais le bip système des applications qui refusent un
/// Ctrl+C sans sélection.
pub const CAPTURE_FIELD: &str = "field";
/// Uniquement la sélection de l'utilisateur.
pub const CAPTURE_SELECTION: &str = "selection";
/// La sélection si elle existe, sinon tout le champ. Pratique, mais commence
/// par une copie spéculative qui fait biper quand il n'y a rien à copier.
pub const CAPTURE_SELECTION_THEN_FIELD: &str = "selectionThenField";

impl Default for AppConfig {
    fn default() -> Self {
        let mut providers = HashMap::new();
        for p in PROVIDERS {
            providers.insert(
                p.to_string(),
                ProviderConfig {
                    api_key: String::new(),
                    model: default_model(p).to_string(),
                    endpoint: default_endpoint(p).to_string(),
                },
            );
        }
        AppConfig {
            shortcuts: default_shortcuts(),
            legacy_shortcut: None,
            default_provider: default_provider(),
            providers,
            preview_before_replace: false,
            target_language: default_language(),
            custom_instructions: String::new(),
            start_minimized: false,
            start_at_login: false,
            capture_mode: CAPTURE_FIELD.to_string(),
            legacy_select_all: None,
        }
    }
}

impl AppConfig {
    /// Complète une config lue sur disque : une version antérieure ne
    /// connaissait qu'un seul raccourci, sous une autre clé.
    fn migrate(&mut self) {
        if self.shortcuts.is_empty() {
            let mut defaults = default_shortcuts();
            if let Some(previous) = self.legacy_shortcut.take() {
                if !previous.trim().is_empty() {
                    // On garde la combinaison que l'utilisateur avait choisie
                    // pour le menu, et on ajoute les deux actions directes.
                    defaults[0].accelerator = previous;
                }
            }
            self.shortcuts = defaults;
        }
        self.legacy_shortcut = None;

        if self.capture_mode.is_empty() {
            self.capture_mode = match self.legacy_select_all {
                // « Prendre tout le champ quand rien n'est sélectionné » devient
                // « toujours tout le champ » : même résultat dans l'usage visé,
                // sans la copie spéculative qui fait biper.
                Some(true) => CAPTURE_FIELD.to_string(),
                Some(false) => CAPTURE_SELECTION.to_string(),
                None => CAPTURE_FIELD.to_string(),
            };
        }
        self.legacy_select_all = None;

        // Une action ajoutée après la dernière sauvegarde de l'utilisateur doit
        // apparaître malgré tout. Si sa combinaison par défaut est déjà prise,
        // on l'ajoute sans raccourci : elle s'affichera vide dans les
        // Paramètres, à l'utilisateur d'en choisir un.
        for default in default_shortcuts() {
            if self.shortcuts.iter().any(|b| b.action == default.action) {
                continue;
            }
            let taken = self
                .shortcuts
                .iter()
                .any(|b| b.accelerator.eq_ignore_ascii_case(&default.accelerator));
            self.shortcuts.push(if taken {
                ShortcutBinding {
                    accelerator: String::new(),
                    action: default.action,
                }
            } else {
                default
            });
        }
    }

    /// Retourne la config d'un fournisseur, en comblant les trous avec les
    /// valeurs par défaut (une config sauvegardée peut être partielle).
    pub fn provider(&self, name: &str) -> ProviderConfig {
        let stored = self.providers.get(name).cloned().unwrap_or_default();
        ProviderConfig {
            api_key: stored.api_key,
            model: if stored.model.is_empty() {
                default_model(name).to_string()
            } else {
                stored.model
            },
            endpoint: if stored.endpoint.is_empty() {
                default_endpoint(name).to_string()
            } else {
                stored.endpoint
            },
        }
    }
}

lazy_static! {
    static ref CONFIG: Mutex<Option<AppConfig>> = Mutex::new(None);
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path_resolver()
        .app_config_dir()
        .ok_or_else(|| "Impossible de résoudre le dossier de configuration".to_string())?;
    fs::create_dir_all(&dir).map_err(|e| format!("Création du dossier de config: {}", e))?;
    Ok(dir)
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join("config.json"))
}

fn history_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join("history.json"))
}

/// Lit la config depuis le disque (une seule fois), puis depuis le cache mémoire.
pub fn get(app: &AppHandle) -> AppConfig {
    let mut cache = CONFIG.lock().unwrap();
    if let Some(cfg) = cache.as_ref() {
        return cfg.clone();
    }
    let mut cfg = config_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<AppConfig>(&raw).ok())
        .unwrap_or_default();
    cfg.migrate();
    *cache = Some(cfg.clone());
    cfg
}

pub fn save(app: &AppHandle, cfg: AppConfig) -> Result<(), String> {
    let path = config_path(app)?;
    let raw = serde_json::to_string_pretty(&cfg)
        .map_err(|e| format!("Sérialisation de la config: {}", e))?;
    fs::write(&path, raw).map_err(|e| format!("Écriture de la config: {}", e))?;
    *CONFIG.lock().unwrap() = Some(cfg);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub original: String,
    pub transformed: String,
    pub action: String,
    pub provider: String,
    pub timestamp: i64,
}

const HISTORY_LIMIT: usize = 200;

pub fn history(app: &AppHandle) -> Vec<HistoryEntry> {
    history_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<Vec<HistoryEntry>>(&raw).ok())
        .unwrap_or_default()
}

pub fn push_history(app: &AppHandle, entry: HistoryEntry) -> Result<(), String> {
    let mut entries = history(app);
    entries.insert(0, entry);
    entries.truncate(HISTORY_LIMIT);
    write_history(app, &entries)
}

pub fn clear_history(app: &AppHandle) -> Result<(), String> {
    write_history(app, &[])
}

fn write_history(app: &AppHandle, entries: &[HistoryEntry]) -> Result<(), String> {
    let path = history_path(app)?;
    let raw = serde_json::to_string_pretty(entries)
        .map_err(|e| format!("Sérialisation de l'historique: {}", e))?;
    fs::write(&path, raw).map_err(|e| format!("Écriture de l'historique: {}", e))
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
