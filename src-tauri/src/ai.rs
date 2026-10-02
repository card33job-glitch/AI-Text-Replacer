use crate::config::AppConfig;
use lazy_static::lazy_static;
use serde_json::{json, Value};
use std::time::Duration;

/// Construit la consigne système envoyée au modèle pour une action donnée.
///
/// Toutes les consignes insistent sur un point : le modèle ne doit répondre
/// qu'avec le texte transformé, puisque sa réponse est collée telle quelle
/// dans l'éditeur de l'utilisateur.
fn system_prompt(action: &str, cfg: &AppConfig) -> String {
    let base = match action {
        "grammar" => "Tu corriges l'orthographe, la grammaire, la conjugaison et la ponctuation du texte fourni. \
Conserve exactement la même langue, le même ton, le même registre et le même sens. \
Ne reformule pas, ne rallonge pas, ne raccourcis pas : corrige uniquement les fautes."
            .to_string(),
        "rephrase" => "Tu reformules le texte fourni pour le rendre plus clair, plus fluide et mieux écrit. \
Conserve la même langue, le même sens et un niveau de longueur comparable."
            .to_string(),
        "professional" => "Tu réécris le texte fourni dans un registre professionnel et courtois, adapté à une communication de travail. \
Conserve la même langue et le même sens. Reste naturel, évite la langue de bois."
            .to_string(),
        "concise" => "Tu réécris le texte fourni de façon nettement plus concise, en gardant la même langue \
et toute l'information essentielle. Supprime les redondances et les formules inutiles."
            .to_string(),
        "summarize" => "Tu résumes le texte fourni en quelques phrases, dans la même langue que le texte d'origine. \
Garde uniquement les informations importantes."
            .to_string(),
        // Seule action où le texte sélectionné n'est pas la matière première
        // mais l'ordre : on produit ce qu'il demande au lieu de le réécrire.
        "prompt" => "Le texte fourni est une CONSIGNE, pas un texte à reformuler. \
Exécute-la et produis ce qu'elle demande : un message, un courriel, un paragraphe, une réponse. \
Adopte un ton naturel et professionnel, adapté à une communication de travail, \
sauf si la consigne demande explicitement autre chose."
            .to_string(),
        "translate" => format!(
            "Tu traduis le texte fourni en {}. Conserve le ton et le registre d'origine. \
Ne commente pas la traduction.",
            cfg.target_language
        ),
        other => format!(
            "Tu appliques la transformation suivante au texte fourni : {}. \
Conserve la langue d'origine sauf indication contraire.",
            other
        ),
    };

    // Les règles de transformation ne s'appliquent pas à une consigne : on ne
    // lui demande ni de conserver la mise en forme, ni de la renvoyer telle
    // quelle si elle est « déjà correcte ».
    let rules = if action == "prompt" {
        "Règles absolues :\n\
         - Réponds UNIQUEMENT avec le texte demandé, prêt à être envoyé tel quel.\n\
         - Ne répète pas la consigne et n'annonce pas ce que tu vas faire (pas de « Voici… »).\n\
         - N'ajoute ni guillemets autour de l'ensemble, ni explication, ni commentaire.\n\
         - Rédige dans la langue de la consigne, sauf si elle en demande une autre."
    } else {
        "Règles absolues :\n\
         - Réponds UNIQUEMENT avec le texte transformé.\n\
         - N'ajoute ni guillemets, ni préambule, ni explication, ni commentaire.\n\
         - Conserve la mise en forme d'origine (sauts de ligne, listes, émojis).\n\
         - Si le texte est déjà correct, renvoie-le tel quel."
    };

    let mut prompt = format!("{}\n\n{}", base, rules);

    if !cfg.custom_instructions.trim().is_empty() {
        prompt.push_str("\n\nConsignes supplémentaires de l'utilisateur :\n");
        prompt.push_str(cfg.custom_instructions.trim());
    }

    prompt
}

lazy_static! {
    /// Client HTTP partagé par toutes les transformations.
    ///
    /// Un `Client` neuf rouvre une connexion et refait toute la poignée de main
    /// TLS. Mesuré sur une liaison ordinaire vers Groq : **~192 ms par appel
    /// avec un client neuf contre ~99 ms en le réutilisant** — la moitié du
    /// temps de connexion, économisée sur chaque correction.
    ///
    /// Le `timeout` n'est pas un détail : l'appel se fait sous le verrou qui
    /// empêche deux transformations simultanées. Une requête qui ne revient
    /// jamais bloquerait tous les raccourcis jusqu'au redémarrage.
    static ref HTTP: reqwest::Client = reqwest::Client::builder()
        // Bien au-delà des 90 s par défaut : garder la connexion ouverte entre
        // deux corrections espacées est exactement le cas d'usage ici.
        .pool_idle_timeout(Duration::from_secs(300))
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
}

pub async fn transform(
    cfg: &AppConfig,
    provider: &str,
    action: &str,
    text: &str,
) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("Aucun texte à transformer".to_string());
    }
    complete(cfg, provider, &system_prompt(action, cfg), text).await
}

/// Un échange avec le modèle : une consigne système, un message, une réponse.
pub async fn complete(
    cfg: &AppConfig,
    provider: &str,
    system: &str,
    text: &str,
) -> Result<String, String> {
    let pc = cfg.provider(provider);
    if provider != "local" && pc.api_key.trim().is_empty() {
        return Err(format!(
            "Aucune clé API configurée pour « {} ». Ouvrez Paramètres pour l'ajouter.",
            provider
        ));
    }

    let client = &*HTTP;

    let response = match provider {
        "claude" => {
            let body = json!({
                "model": pc.model,
                "max_tokens": 4096,
                "system": system,
                "messages": [{ "role": "user", "content": text }],
            });
            client
                .post(&pc.endpoint)
                .header("x-api-key", pc.api_key.trim())
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
        }
        // OpenAI, Groq et les serveurs locaux (Ollama, LM Studio) partagent
        // le même format de requête et de réponse.
        _ => {
            let body = json!({
                "model": pc.model,
                "temperature": 0.3,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": text },
                ],
            });
            let mut req = client.post(&pc.endpoint).json(&body);
            if !pc.api_key.trim().is_empty() {
                req = req.header("Authorization", format!("Bearer {}", pc.api_key.trim()));
            }
            req.send().await
        }
    };

    let response = response.map_err(|e| format!("Requête vers {} échouée: {}", provider, e))?;
    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|e| format!("Réponse illisible de {}: {}", provider, e))?;

    if !status.is_success() {
        let detail = payload
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("erreur inconnue");
        return Err(format!("{} a répondu {}: {}", provider, status, detail));
    }

    let out = if provider == "claude" {
        payload
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        payload
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(str::to_string)
    };

    out.map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Réponse vide de {}", provider))
}
