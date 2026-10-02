//! Proposition de réponse à un message reçu.
//!
//! Le modèle reçoit trois sources, chacune avec son rôle :
//! - des passages de Confluence, pour le fond (procédures, liens, règles) ;
//! - les réponses que l'utilisateur a déjà envoyées à des messages proches,
//!   pour le fond déjà validé par lui et pour sa façon de répondre ;
//! - des textes qu'il a écrits (corrigés par l'application), pour son style.
//!
//! Les réponses passées viennent d'ici même : chaque proposition que
//! l'utilisateur copie ou colle, après ses éventuelles retouches, est retenue.
//! Plus il s'en sert, mieux l'outil lui ressemble.

use crate::config::{self, AppConfig};
use crate::search::{tokenize, Bm25};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;

/// Réponses retenues au-delà desquelles les plus anciennes sont oubliées.
const MEMORY_LIMIT: usize = 2000;
const MAX_PASSAGES: usize = 6;
const MAX_PAST_REPLIES: usize = 4;
const MAX_STYLE_SAMPLES: usize = 3;
/// Un échange passé démesuré (fil de courriels collé) mangerait le prompt.
const MAX_EXAMPLE_CHARS: usize = 1500;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Remembered {
    pub message: String,
    pub reply: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub text: String,
    /// Pages Confluence mises sous les yeux du modèle, sans doublon.
    pub sources: Vec<Source>,
    /// Nombre de réponses passées fournies en exemple.
    pub past_replies: usize,
}

fn memory_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config::config_dir(app)?.join("replies.json"))
}

pub fn memory(app: &AppHandle) -> Vec<Remembered> {
    memory_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<Vec<Remembered>>(&raw).ok())
        .unwrap_or_default()
}

fn write_memory(app: &AppHandle, entries: &[Remembered]) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(entries)
        .map_err(|e| format!("Sérialisation des réponses: {}", e))?;
    fs::write(memory_path(app)?, raw).map_err(|e| format!("Écriture des réponses: {}", e))
}

/// Retient une réponse que l'utilisateur a choisi d'envoyer.
pub fn remember(app: &AppHandle, message: String, reply: String) -> Result<(), String> {
    if message.trim().is_empty() || reply.trim().is_empty() {
        return Ok(());
    }
    let mut entries = memory(app);
    // La même réponse copiée puis collée ne doit compter qu'une fois.
    entries.retain(|e| !(e.message == message && e.reply == reply));
    entries.insert(
        0,
        Remembered {
            message,
            reply,
            timestamp: config::now_millis(),
        },
    );
    entries.truncate(MEMORY_LIMIT);
    write_memory(app, &entries)
}

pub fn clear_memory(app: &AppHandle) -> Result<(), String> {
    write_memory(app, &[])
}

fn clip(text: &str) -> String {
    let text = text.trim();
    match text.char_indices().nth(MAX_EXAMPLE_CHARS) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

/// Les `limit` textes les plus proches de la requête. Sans aucun terme commun,
/// on retombe sur les plus récents : même hors sujet, ils montrent le ton.
fn closest<'a>(query: &[String], texts: &[&'a str], limit: usize) -> Vec<usize> {
    if texts.is_empty() {
        return Vec::new();
    }
    let index = Bm25::new(texts.iter().map(|t| tokenize(t)));
    let hits: Vec<usize> = index.search(query, limit).into_iter().map(|h| h.0).collect();
    if hits.is_empty() {
        (0..texts.len().min(limit.min(2))).collect()
    } else {
        hits
    }
}

const SYSTEM: &str = "Tu rédiges, au nom de l'utilisateur, la réponse à un message qu'il a reçu \
(Teams, courriel, ticket…).

Le texte placé sous « MESSAGE » est le plus souvent le message reçu auquel répondre. \
Il peut aussi être un brouillon ou une consigne de l'utilisateur décrivant la réponse voulue : \
dans ce cas, rédige cette réponse.

Pour le fond, appuie-toi sur la DOCUMENTATION (extraits du Confluence de l'entreprise) \
et sur les RÉPONSES PASSÉES de l'utilisateur, qu'il a lui-même validées. \
Pour la forme — ton, longueur, tutoiement ou vouvoiement, formules d'ouverture et de clôture — \
imite ses RÉPONSES PASSÉES et ses EXEMPLES DE STYLE.

N'invente jamais un fait, une procédure, un lien, un nom ou un chiffre absent de ces sources. \
Si elles ne suffisent pas, écris une réponse brève qui dit ce que tu dois vérifier ou demande \
la précision qui manque, plutôt que de deviner. Tu peux donner le lien d'une page Confluence \
quand il aide le destinataire.

Règles absolues :
- Réponds UNIQUEMENT avec le texte de la réponse, prêt à être envoyé tel quel.
- Pas de préambule (« Voici… »), pas de guillemets autour, pas de commentaire.
- Ne mentionne pas les « extraits », la « documentation fournie » ni leurs numéros.
- Rédige dans la langue du message.";

/// Assemble le contexte et demande la réponse au modèle.
///
/// `hint` est une consigne ponctuelle ajoutée depuis la popup (« plus court »,
/// « tutoie-le ») pour régénérer sans tout retaper.
pub async fn suggest(
    app: &AppHandle,
    cfg: &AppConfig,
    provider: &str,
    message: &str,
    hint: Option<&str>,
) -> Result<Suggestion, String> {
    if message.trim().is_empty() {
        return Err("Aucun message auquel répondre.".to_string());
    }
    let query = tokenize(message);

    let passages = crate::confluence::search(app, message, MAX_PASSAGES);

    let remembered = memory(app);
    let remembered_texts: Vec<&str> = remembered.iter().map(|r| r.message.as_str()).collect();
    let past: Vec<&Remembered> = closest(&query, &remembered_texts, MAX_PAST_REPLIES)
        .into_iter()
        .map(|i| &remembered[i])
        .collect();

    // Ce que l'application a corrigé ou reformulé est, à la retouche près, ce
    // que l'utilisateur a écrit. Les sorties de « Traduire » ou « Résumer »
    // n'ont pas sa voix, et les réponses sont déjà dans la mémoire.
    let history = config::history(app);
    let written: Vec<&str> = history
        .iter()
        .filter(|h| matches!(h.action.as_str(), "grammar" | "rephrase" | "professional" | "concise"))
        .map(|h| h.transformed.as_str())
        .filter(|t| t.chars().count() >= 40)
        .collect();
    let style: Vec<&str> = closest(&query, &written, MAX_STYLE_SAMPLES)
        .into_iter()
        .map(|i| written[i])
        .collect();

    let mut prompt = String::new();
    if !passages.is_empty() {
        prompt.push_str("DOCUMENTATION :\n\n");
        for (i, p) in passages.iter().enumerate() {
            prompt.push_str(&format!("[{}] {} — {}\n{}\n\n", i + 1, p.title, p.url, p.text.trim()));
        }
    }
    if !past.is_empty() {
        prompt.push_str("RÉPONSES PASSÉES :\n\n");
        for r in &past {
            prompt.push_str(&format!(
                "Message reçu :\n{}\nSa réponse :\n{}\n---\n",
                clip(&r.message),
                clip(&r.reply)
            ));
        }
        prompt.push('\n');
    }
    if !style.is_empty() {
        prompt.push_str("EXEMPLES DE STYLE (textes qu'il a écrits) :\n\n");
        for s in &style {
            prompt.push_str(&format!("{}\n---\n", clip(s)));
        }
        prompt.push('\n');
    }
    if let Some(hint) = hint.map(str::trim).filter(|h| !h.is_empty()) {
        prompt.push_str(&format!("CONSIGNE POUR CETTE RÉPONSE :\n{}\n\n", hint));
    }
    prompt.push_str("MESSAGE :\n");
    prompt.push_str(message.trim());

    let mut system = SYSTEM.to_string();
    if !cfg.custom_instructions.trim().is_empty() {
        system.push_str("\n\nConsignes supplémentaires de l'utilisateur :\n");
        system.push_str(cfg.custom_instructions.trim());
    }

    let text = crate::ai::complete(cfg, provider, &system, &prompt).await?;

    let mut sources: Vec<Source> = Vec::new();
    for p in passages {
        if !sources.iter().any(|s| s.url == p.url && s.title == p.title) {
            sources.push(Source {
                title: p.title,
                url: p.url,
            });
        }
    }

    Ok(Suggestion {
        text,
        sources,
        past_replies: past.len(),
    })
}
