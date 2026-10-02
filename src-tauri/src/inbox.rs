//! Lecture du message auquel répondre, directement dans l'interface de
//! l'application cible, via l'accessibilité Windows (UI Automation).
//!
//! Le presse-papiers ne suffit pas ici. Sans sélection, `Ctrl+A` dans un fil
//! de discussion sélectionne toute la page, et dans la liste des messages
//! d'Outlook `Ctrl+C` copie l'élément de courriel, pas du texte. L'arbre
//! d'accessibilité, lui, donne le corps du courriel ouvert ou les messages
//! d'une conversation, sans rien toucher à l'écran.

/// Texte sélectionné dans l'élément qui a le focus, lu sans presse-papiers.
///
/// `Ok(None)` : l'élément n'est pas un texte (une liste de courriels, par
/// exemple), et un `Ctrl+C` n'y copierait pas une sélection de texte.
/// `Ok(Some(""))` : c'est un texte, sans sélection. `Err` : l'application
/// ne publie rien d'exploitable, il faut se rabattre sur le presse-papiers.
pub fn focused_selection() -> Result<Option<String>, String> {
    platform::focused_selection()
}

/// Ce que l'application cible donne à lire.
enum Source {
    Outlook(Email),
    Teams(Vec<ChatMessage>),
}

/// Courriel ouvert dans Outlook.
pub struct Email {
    /// « De : … » et « Objet : … », déjà mis en forme.
    pub header: String,
    /// Corps tel que livré, fil cité compris.
    pub body: String,
}

fn read_source(window: isize, app_name: &str) -> Option<Source> {
    let app = app_name.to_ascii_lowercase();
    if app.starts_with("outlook") {
        platform::outlook_email(window).map(Source::Outlook)
    } else if app.starts_with("ms-teams") || app.starts_with("teams") {
        // Chromium ne construit son arbre d'accessibilité qu'à la première
        // interrogation d'un client : la toute première lecture peut revenir
        // vide.
        let mut messages = platform::teams_messages(window);
        if messages.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(400));
            messages = platform::teams_messages(window);
        }
        (!messages.is_empty()).then_some(Source::Teams(messages))
    } else {
        None
    }
}

/// Bulles de la conversation Teams ouverte dans `window`.
pub fn chat_messages(window: isize) -> Vec<ChatMessage> {
    platform::teams_messages(window)
}

/// Dernier message reçu dans la fenêtre `window`, suivi de la conversation
/// qui l'entoure : le courriel ouvert dans Outlook, le dernier message
/// d'autrui dans une conversation Teams.
pub fn last_received(window: isize, app_name: &str) -> Option<String> {
    let text = match read_source(window, app_name)? {
        Source::Outlook(email) => {
            let (latest, thread) = split_first_message(&email.body);
            with_thread(&format!("{}{}", email.header, latest), &thread)
        }
        Source::Teams(messages) => last_received_chat(&messages)?,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| truncate(text, MAX_MESSAGE_CHARS))
}

/// Le passage sélectionné, accompagné de la conversation où il a été pris :
/// les derniers messages de Teams, le courriel ouvert dans Outlook. Sans
/// cela, « ok pour jeudi ? » ne dit pas de quoi l'on parle.
pub fn with_context(selection: &str, window: isize, app_name: &str) -> String {
    let context = match read_source(window, app_name) {
        Some(Source::Outlook(email)) => format!("{}{}", email.header, normalize(&email.body)),
        Some(Source::Teams(messages)) => recent_chat(&messages),
        None => String::new(),
    };
    // Tout le courriel sélectionné : le contexte n'apprendrait rien.
    if context.trim() == selection.trim() {
        return selection.to_string();
    }
    with_thread(selection, &context)
}

/// Un long fil cité ne doit pas noyer la recherche ni le prompt.
const MAX_MESSAGE_CHARS: usize = 6000;

fn truncate(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    }
}

/// Le moteur Word d'Outlook sépare les paragraphes par `\r` et met une espace
/// insécable avant les deux-points (« De\u{a0}: »).
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace(['\r', '\u{b}'], "\n")
        .replace('\u{a0}', " ")
}

/// Conversation gardée sous le message, pour que le modèle sache à quoi il
/// répond. Dix bulles de Teams y tiennent largement, un long fil de
/// courriels y est coupé.
const MAX_THREAD_CHARS: usize = 5000;

/// Le message à traiter, suivi de la conversation qui l'entoure sous un
/// intitulé que le prompt de réponse connaît.
pub fn with_thread(latest: &str, thread: &str) -> String {
    let thread = thread.trim();
    if thread.is_empty() {
        latest.trim().to_string()
    } else {
        format!(
            "{}\n\n{}\n{}",
            latest.trim(),
            crate::reply::THREAD_MARKER,
            truncate(thread, MAX_THREAD_CHARS)
        )
    }
}

/// Vrai si `lines[i]` ouvre l'en-tête d'un message cité (« De : … » suivi
/// d'« Envoyé : … », « Le … a écrit : », « -----Message d'origine----- »).
fn is_quote_header(lines: &[&str], i: usize) -> bool {
    let line = lines[i].trim();
    let lower = line.to_lowercase();
    if lower.starts_with("-----") && (lower.contains("origin") || lower.contains("message")) {
        return true;
    }
    if (lower.starts_with("le ") && lower.ends_with("a écrit :"))
        || (lower.starts_with("on ") && lower.ends_with("wrote:"))
    {
        return true;
    }
    let opens = ["de :", "de:", "from:", "from :"]
        .iter()
        .any(|p| lower.starts_with(p));
    if !opens {
        return false;
    }
    // Un « De : » seul peut être une phrase ; l'en-tête est suivi de la date.
    lines
        .iter()
        .skip(i + 1)
        .take(4)
        .map(|l| l.trim().to_lowercase())
        .any(|l| l.starts_with("envoy") || l.starts_with("sent") || l.starts_with("date"))
}

/// Premier message non vide d'un courriel, et le fil cité qui le suit. Le
/// premier message est le dernier reçu quand on lit un courriel ouvert, le
/// message cité quand on est en train d'y répondre (le brouillon, encore
/// vide, vient en tête).
pub fn split_first_message(body: &str) -> (String, String) {
    let body = normalize(body);
    let lines: Vec<&str> = body.lines().collect();
    let mut start = 0;
    for i in 0..=lines.len() {
        if i == lines.len() || (i > start && is_quote_header(&lines, i)) {
            let segment = lines[start..i].join("\n");
            if has_content(&segment) {
                let rest = lines[i..].join("\n");
                return (segment.trim().to_string(), rest.trim().to_string());
            }
            start = i;
        }
    }
    (body.trim().to_string(), String::new())
}

/// Un segment fait d'un en-tête cité sans corps n'est pas un message.
fn has_content(segment: &str) -> bool {
    let lines: Vec<&str> = segment.lines().collect();
    let skip_header = if !lines.is_empty() && is_quote_header(&lines, 0) {
        // En-tête : « De », « Envoyé », « À », « Cc », « Objet ».
        lines
            .iter()
            .take_while(|l| {
                let l = l.trim().to_lowercase();
                ["de", "from", "envoy", "sent", "date", "à", "a :", "to", "cc", "objet", "subject", "-----"]
                    .iter()
                    .any(|p| l.starts_with(p))
            })
            .count()
    } else {
        0
    };
    lines.iter().skip(skip_header).any(|l| !l.trim().is_empty())
}

/// Une bulle de conversation telle que l'accessibilité la décrit.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub mine: bool,
    /// Auteur et date (« Huneault, Carl 30 septembre 2026 13:55 »).
    pub header: String,
    pub text: String,
}

impl ChatMessage {
    /// `name` est le nom accessible de la bulle (« <texte> <auteur> <date>. »,
    /// parfois précédé d'un état comme « Envoyé »), `content` son texte seul.
    pub fn from_accessible(mine: bool, name: &str, content: &str) -> Self {
        let content = content.trim();
        let name = name.trim();
        let header = if content.is_empty() {
            String::new()
        } else {
            name.rfind(content)
                .map(|i| name[i + content.len()..].trim())
                .unwrap_or("")
                .trim_end_matches('.')
                .to_string()
        };
        let text = if content.is_empty() { name } else { content };
        ChatMessage { mine, header, text: text.to_string() }
    }

    fn line(&self) -> String {
        let who = if self.mine { "Moi" } else { self.header.as_str() };
        if who.is_empty() {
            self.text.clone()
        } else {
            format!("{} : {}", who, self.text)
        }
    }
}

/// Bulles de conversation données en contexte.
const MAX_THREAD_MESSAGES: usize = 10;

fn lines(messages: &[&ChatMessage]) -> String {
    messages.iter().map(|m| m.line()).collect::<Vec<_>>().join("\n")
}

/// Les dix dernières bulles de la conversation, dans l'ordre.
pub fn recent_chat(messages: &[ChatMessage]) -> String {
    let messages: Vec<&ChatMessage> = messages.iter().filter(|m| !m.text.trim().is_empty()).collect();
    lines(&messages[messages.len().saturating_sub(MAX_THREAD_MESSAGES)..])
}

/// Dernier message reçu d'une conversation, suivi de la conversation qui
/// l'entoure : jusqu'à dix bulles avant lui, et ce que l'utilisateur a
/// écrit depuis.
///
/// Une personne écrit souvent en plusieurs bulles d'affilée : le « message »
/// est la dernière suite de bulles reçues, regroupées. Si l'utilisateur a
/// déjà répondu ensuite, c'est quand même ce message-là qu'on retient : il
/// demande une réponse, peut-être pour compléter la sienne — et le modèle
/// doit alors savoir ce qui a déjà été dit.
pub fn last_received_chat(messages: &[ChatMessage]) -> Option<String> {
    let messages: Vec<&ChatMessage> = messages.iter().filter(|m| !m.text.trim().is_empty()).collect();
    let end = messages.iter().rposition(|m| !m.mine)? + 1;
    let start = messages[..end]
        .iter()
        .rposition(|m| m.mine)
        .map_or(0, |i| i + 1);

    let latest = lines(&messages[start..end]);
    let before = lines(&messages[start.saturating_sub(MAX_THREAD_MESSAGES)..start]);
    let after = lines(&messages[end..]);
    let thread = if after.is_empty() {
        before
    } else {
        format!("{}\n(après ce message)\n{}", before, after)
    };
    Some(with_thread(&latest, &thread))
}

#[cfg(target_os = "windows")]
mod platform {
    use uiautomation::controls::ControlType;
    use uiautomation::patterns::UITextPattern;
    use uiautomation::types::{Handle, TreeScope, UIProperty};
    use uiautomation::variants::Variant;
    use uiautomation::{UIAutomation, UIElement};

    fn automation() -> Option<UIAutomation> {
        // `new` initialise COM sur le thread ; s'il l'est déjà dans un autre
        // mode, on utilise l'initialisation existante.
        UIAutomation::new().or_else(|_| UIAutomation::new_direct()).ok()
    }

    fn text_of(element: &UIElement) -> Option<String> {
        let pattern = element.get_pattern::<UITextPattern>().ok()?;
        pattern.get_document_range().ok()?.get_text(-1).ok()
    }

    pub fn focused_selection() -> Result<Option<String>, String> {
        let automation = automation().ok_or("UI Automation indisponible")?;
        let focused = automation
            .get_focused_element()
            .map_err(|e| e.to_string())?;
        let control = focused.get_control_type().map_err(|e| e.to_string())?;
        if matches!(
            control,
            ControlType::List
                | ControlType::ListItem
                | ControlType::DataGrid
                | ControlType::DataItem
                | ControlType::Tree
                | ControlType::TreeItem
                | ControlType::Table
        ) {
            return Ok(None);
        }
        let pattern = focused
            .get_pattern::<UITextPattern>()
            .map_err(|e| e.to_string())?;
        let ranges = pattern.get_selection().map_err(|e| e.to_string())?;
        let text = ranges
            .iter()
            .filter_map(|r| r.get_text(-1).ok())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Some(text))
    }

    fn text_elements(automation: &UIAutomation, root: &UIElement) -> Vec<UIElement> {
        let Ok(condition) = automation.create_property_condition(
            UIProperty::IsTextPatternAvailable,
            Variant::from(true),
            None,
        ) else {
            return Vec::new();
        };
        root.find_all(TreeScope::Descendants, &condition)
            .unwrap_or_default()
    }

    /// Outlook classique : le corps du courriel ouvert est un `Document`
    /// (moteur Word), l'expéditeur et l'objet des champs nommés à côté.
    pub fn outlook_email(window: isize) -> Option<super::Email> {
        let automation = automation()?;
        let root = automation.element_from_handle(Handle::from(window)).ok()?;

        let mut body = String::new();
        let mut sender = String::new();
        let mut subject = String::new();
        for element in text_elements(&automation, &root) {
            let name = element.get_name().unwrap_or_default();
            match element.get_control_type() {
                Ok(ControlType::Document) => {
                    // Plusieurs documents possibles (aperçu et réponse en
                    // ligne) : le plus long porte le fil complet.
                    if let Some(text) = text_of(&element) {
                        if text.len() > body.len() {
                            body = text;
                        }
                    }
                }
                Ok(ControlType::Edit) if sender.is_empty() && (name == "De" || name == "From") => {
                    sender = text_of(&element).unwrap_or_default();
                }
                Ok(ControlType::Edit)
                    if subject.is_empty() && (name == "Objet" || name == "Subject") =>
                {
                    subject = text_of(&element).unwrap_or_default();
                }
                _ => {}
            }
        }
        if body.trim().is_empty() {
            return None;
        }

        let mut header = String::new();
        // Le champ « De » ajoute la présence (« Dufort, Patrick, Disponible ») :
        // seul le nom intéresse.
        let sender = sender.trim();
        if !sender.is_empty() {
            let name = sender.splitn(3, ", ").take(2).collect::<Vec<_>>().join(", ");
            header.push_str(&format!("De : {}\n", name));
        }
        if !subject.trim().is_empty() {
            header.push_str(&format!("Objet : {}\n", subject.trim()));
        }
        if !header.is_empty() {
            header.push('\n');
        }
        Some(super::Email { header, body })
    }

    /// Nouveau Teams (WebView2) : chaque bulle est un `Group` d'identifiant
    /// `message-body-<id>`, de classe `fui-ChatMessage__body` pour un message
    /// reçu et `fui-ChatMyMessage__body` pour un message envoyé. Son nom
    /// enchaîne texte, auteur et date ; le texte seul est dans l'enfant
    /// `content-<id>`. Seuls les messages chargés à l'écran sont visibles.
    pub fn teams_messages(window: isize) -> Vec<super::ChatMessage> {
        let Some(automation) = automation() else {
            return Vec::new();
        };
        let Ok(root) = automation.element_from_handle(Handle::from(window)) else {
            return Vec::new();
        };
        let Ok(groups) = automation.create_property_condition(
            UIProperty::ControlType,
            Variant::from(ControlType::Group as i32),
            None,
        ) else {
            return Vec::new();
        };

        let mut messages = Vec::new();
        for element in root.find_all(TreeScope::Descendants, &groups).unwrap_or_default() {
            let id = element.get_automation_id().unwrap_or_default();
            let Some(key) = id.strip_prefix("message-body-") else {
                continue;
            };
            let class = element.get_classname().unwrap_or_default();
            let mine = class.contains("ChatMyMessage");
            let name = element.get_name().unwrap_or_default();
            let content = automation
                .create_property_condition(
                    UIProperty::AutomationId,
                    Variant::from(format!("content-{}", key)),
                    None,
                )
                .ok()
                .and_then(|c| element.find_first(TreeScope::Descendants, &c).ok())
                .and_then(|c| c.get_name().ok())
                .unwrap_or_default();
            messages.push(super::ChatMessage::from_accessible(mine, &name, &content));
        }
        messages
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    pub fn focused_selection() -> Result<Option<String>, String> {
        Err("UI Automation propre à Windows".to_string())
    }
    pub fn outlook_email(_window: isize) -> Option<super::Email> {
        None
    }
    pub fn teams_messages(_window: isize) -> Vec<super::ChatMessage> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::split_first_message;

    fn first_message(body: &str) -> String {
        split_first_message(body).0
    }

    #[test]
    fn courriel_lu_garde_le_dernier_message() {
        // Texte tel que livré par Outlook : `\r` et espaces insécables.
        let body = "Tentative de connexion effectuée\r\rPatrick\r\rDe\u{a0}: Cardin, Steve <steve@x.com>\rEnvoyé\u{a0}: 2 octobre 2026 08:44\rÀ\u{a0}: Dufort, Patrick\rObjet\u{a0}: Accès\r\rVoici les liens";
        let (latest, thread) = split_first_message(body);
        assert_eq!(latest, "Tentative de connexion effectuée\n\nPatrick");
        assert!(thread.starts_with("De : Cardin, Steve"));
        assert!(thread.ends_with("Voici les liens"));
    }

    #[test]
    fn reponse_en_cours_prend_le_message_cite() {
        let body = "\r\rDe : Dufort, Patrick\rEnvoyé : 2 octobre 2026 09:00\rÀ : Cardin, Steve\rObjet : RE: Accès\r\rÇa ne marche pas\r\rDe : Cardin, Steve\rEnvoyé : 2 octobre 2026 08:44\r\rVoici";
        let (latest, thread) = split_first_message(body);
        assert!(latest.contains("Ça ne marche pas"));
        assert!(!latest.contains("Voici"));
        assert!(thread.contains("Voici"));
    }

    #[test]
    fn teams_regroupe_les_bulles_recues() {
        use super::{last_received_chat, ChatMessage};
        let messages = vec![
            ChatMessage::from_accessible(true, "Que fais-tu avec la carte ? Cardin, Steve 30 septembre 2026 13:54.", "Que fais-tu avec la carte ?"),
            ChatMessage::from_accessible(false, "juste changer le statut Huneault, Carl 30 septembre 2026 13:55.", "juste changer le statut"),
            ChatMessage::from_accessible(false, "et commenter Huneault, Carl 30 septembre 2026 13:56.", "et commenter"),
            ChatMessage::from_accessible(true, "Envoyé Merci Cardin, Steve Aujourd'hui à 11:05.", "Merci"),
        ];
        let text = last_received_chat(&messages).unwrap();
        let (latest, thread) = text.split_once(crate::reply::THREAD_MARKER).unwrap();
        assert_eq!(
            latest.trim(),
            "Huneault, Carl 30 septembre 2026 13:55 : juste changer le statut\n\
             Huneault, Carl 30 septembre 2026 13:56 : et commenter"
        );
        assert_eq!(
            thread.trim(),
            "Moi : Que fais-tu avec la carte ?\n(après ce message)\nMoi : Merci"
        );
    }

    #[test]
    fn contexte_des_dix_dernieres_bulles() {
        use super::{recent_chat, ChatMessage};
        let messages: Vec<ChatMessage> = (1..=12)
            .map(|i| ChatMessage::from_accessible(i % 2 == 0, &format!("m{} X 08:00.", i), &format!("m{}", i)))
            .collect();
        let context = recent_chat(&messages);
        assert_eq!(context.lines().count(), 10);
        assert!(context.starts_with("X 08:00 : m3"));
        assert!(context.ends_with("Moi : m12"));
    }

    #[test]
    fn teams_sans_message_recu() {
        use super::{last_received_chat, ChatMessage};
        let messages = vec![ChatMessage::from_accessible(true, "Salut Cardin, Steve 08:48.", "Salut")];
        assert!(last_received_chat(&messages).is_none());
    }

    #[test]
    fn de_dans_une_phrase_ne_coupe_pas() {
        let body = "De : mon côté, tout fonctionne.\rMerci";
        assert_eq!(first_message(body), "De : mon côté, tout fonctionne.\nMerci");
    }
}
