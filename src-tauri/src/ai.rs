use crate::config::AppConfig;
use serde_json::{json, Value};

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

pub async fn transform(
    cfg: &AppConfig,
    provider: &str,
    action: &str,
    text: &str,
) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("Aucun texte à transformer".to_string());
    }

    let pc = cfg.provider(provider);
    if provider != "local" && pc.api_key.trim().is_empty() {
        return Err(format!(
            "Aucune clé API configurée pour « {} ». Ouvrez Paramètres pour l'ajouter.",
            provider
        ));
    }

    let system = system_prompt(action, cfg);
    let client = reqwest::Client::new();

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
