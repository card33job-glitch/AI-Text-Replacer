//! Copie locale du site Confluence, et recherche dans cette copie.
//!
//! Interroger Confluence à chaque demande de réponse serait lent (plusieurs
//! allers-retours) et sa recherche CQL cherche des expressions, pas des
//! passages pertinents. On télécharge donc toutes les pages une fois, en texte
//! brut, et on les indexe localement : une proposition de réponse ne coûte
//! ensuite aucune requête à Confluence.

use crate::config::{self, AppConfig};
use crate::search::{tokenize, Bm25};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Longueur visée d'un passage. Assez court pour que plusieurs passages de
/// pages différentes tiennent dans le prompt, assez long pour garder une
/// procédure lisible d'un seul tenant.
const CHUNK_CHARS: usize = 1200;
/// Au-delà, la copie locale est jugée périmée par la synchronisation automatique.
const STALE_AFTER_MS: i64 = 24 * 3600 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub title: String,
    pub space: String,
    pub url: String,
    pub text: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    synced_at: Option<i64>,
    pages: Vec<Page>,
}

/// Passage retrouvé pour une requête.
#[derive(Debug, Clone)]
pub struct Passage {
    pub title: String,
    pub url: String,
    pub text: String,
}

struct Index {
    synced_at: Option<i64>,
    pages: Vec<Page>,
    /// (rang de la page, début et fin du passage dans son texte)
    chunks: Vec<(usize, usize, usize)>,
    bm25: Bm25,
}

impl Index {
    fn build(store: Store) -> Self {
        let mut chunks = Vec::new();
        let mut terms = Vec::new();
        for (page_idx, page) in store.pages.iter().enumerate() {
            let title_terms = tokenize(&page.title);
            for (start, end) in split_chunks(&page.text) {
                // Le titre compte dans chaque passage : « VPN » dans le titre
                // d'une page dit plus que « VPN » cité en passant.
                let mut t = title_terms.clone();
                t.extend(title_terms.iter().cloned());
                t.extend(tokenize(&page.text[start..end]));
                terms.push(t);
                chunks.push((page_idx, start, end));
            }
        }
        Index {
            synced_at: store.synced_at,
            pages: store.pages,
            chunks,
            bm25: Bm25::new(terms),
        }
    }
}

lazy_static! {
    static ref INDEX: Mutex<Option<Arc<Index>>> = Mutex::new(None);
    static ref HTTP: reqwest::Client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
}

static SYNCING: AtomicBool = AtomicBool::new(false);

fn store_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config::config_dir(app)?.join("confluence.json"))
}

/// Charge l'index depuis le disque au premier usage, puis le garde en mémoire.
fn index(app: &AppHandle) -> Arc<Index> {
    let mut cache = INDEX.lock().unwrap();
    if let Some(index) = cache.as_ref() {
        return index.clone();
    }
    let store = store_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<Store>(&raw).ok())
        .unwrap_or_default();
    let index = Arc::new(Index::build(store));
    *cache = Some(index.clone());
    index
}

/// Construit l'index en arrière-plan, pour que la première proposition de
/// réponse n'ait pas à attendre la lecture de plusieurs mégaoctets.
pub fn preload(app: AppHandle) {
    std::thread::spawn(move || {
        index(&app);
    });
}

/// Les passages les plus proches de `query`, au plus `limit`, jamais deux fois
/// le même passage.
pub fn search(app: &AppHandle, query: &str, limit: usize) -> Vec<Passage> {
    let index = index(app);
    index
        .bm25
        .search(&tokenize(query), limit)
        .into_iter()
        .map(|(i, _)| {
            let (page_idx, start, end) = index.chunks[i];
            let page = &index.pages[page_idx];
            Passage {
                title: page.title.clone(),
                url: page.url.clone(),
                text: page.text[start..end].to_string(),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub pages: usize,
    pub synced_at: Option<i64>,
    pub syncing: bool,
}

pub fn status(app: &AppHandle) -> Status {
    let index = index(app);
    Status {
        pages: index.pages.len(),
        synced_at: index.synced_at,
        syncing: SYNCING.load(Ordering::SeqCst),
    }
}

/// Synchronise au démarrage si la copie locale est absente ou périmée.
pub fn sync_if_stale(app: AppHandle) {
    let cfg = config::get(&app);
    if !cfg.confluence.auto_sync || !cfg.confluence.is_configured() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let synced_at = index(&app).synced_at.unwrap_or(0);
        if config::now_millis() - synced_at < STALE_AFTER_MS {
            return;
        }
        if let Err(e) = sync(&app).await {
            eprintln!("Synchronisation Confluence: {}", e);
        }
    });
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    pages: usize,
}

/// Télécharge toutes les pages des espaces configurés et remplace la copie
/// locale. En cas d'échec, l'ancienne copie reste en place.
pub async fn sync(app: &AppHandle) -> Result<Status, String> {
    if SYNCING.swap(true, Ordering::SeqCst) {
        return Err("Une synchronisation est déjà en cours.".to_string());
    }
    let outcome = download(app).await;
    SYNCING.store(false, Ordering::SeqCst);

    let pages = outcome?;
    let store = Store {
        synced_at: Some(config::now_millis()),
        pages,
    };
    let raw = serde_json::to_string(&store)
        .map_err(|e| format!("Sérialisation de la copie Confluence: {}", e))?;
    fs::write(store_path(app)?, raw)
        .map_err(|e| format!("Écriture de la copie Confluence: {}", e))?;

    *INDEX.lock().unwrap() = Some(Arc::new(Index::build(store)));
    Ok(status(app))
}

async fn download(app: &AppHandle) -> Result<Vec<Page>, String> {
    let cfg: AppConfig = config::get(app);
    let cc = &cfg.confluence;
    if !cc.is_configured() {
        return Err("Renseignez l'adresse de Confluence et un jeton d'accès.".to_string());
    }
    let base = cc.base_url.trim().trim_end_matches('/').to_string();

    let spaces: Vec<Option<String>> = {
        let keys: Vec<String> = cc
            .spaces
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if keys.is_empty() {
            vec![None]
        } else {
            keys.into_iter().map(Some).collect()
        }
    };

    let mut pages = Vec::new();
    for space in spaces {
        let mut url = format!(
            "{}/rest/api/content?type=page&status=current&limit=50&expand=body.storage,space",
            base
        );
        if let Some(key) = &space {
            url.push_str(&format!("&spaceKey={}", urlencode(key)));
        }

        loop {
            let payload = get_json(cc, &url).await?;
            let results = payload
                .get("results")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            // `_links.base` inclut le chemin de contexte (« /wiki » sur Cloud),
            // que les liens relatifs de la réponse supposent.
            let links_base = payload
                .pointer("/_links/base")
                .and_then(Value::as_str)
                .unwrap_or(&base)
                .trim_end_matches('/')
                .to_string();

            for item in &results {
                let storage = item
                    .pointer("/body/storage/value")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let text = storage_to_text(storage);
                if text.trim().is_empty() {
                    continue;
                }
                let webui = item
                    .pointer("/_links/webui")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                pages.push(Page {
                    id: item.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                    title: item.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
                    space: item
                        .pointer("/space/key")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    url: if webui.is_empty() {
                        String::new()
                    } else {
                        format!("{}{}", links_base, webui)
                    },
                    text,
                });
            }

            if let Some(window) = app.get_window("main") {
                let _ = window.emit("confluence-sync-progress", Progress { pages: pages.len() });
            }

            match payload.pointer("/_links/next").and_then(Value::as_str) {
                Some(next) if !results.is_empty() => url = format!("{}{}", links_base, next),
                _ => break,
            }
        }
    }
    Ok(pages)
}

async fn get_json(cc: &config::ConfluenceConfig, url: &str) -> Result<Value, String> {
    let token = cc.api_token.trim();
    let mut request = HTTP.get(url).header("Accept", "application/json");
    request = if cc.email.trim().is_empty() {
        request.bearer_auth(token)
    } else {
        request.basic_auth(cc.email.trim(), Some(token))
    };

    let response = request
        .send()
        .await
        .map_err(|e| format!("Confluence injoignable: {}", e))?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(format!(
            "Confluence refuse l'accès ({}). Vérifiez l'adresse courriel et le jeton.",
            status
        ));
    }
    if !status.is_success() {
        return Err(format!("Confluence a répondu {} pour {}", status, url));
    }
    response
        .json()
        .await
        .map_err(|e| format!("Réponse illisible de Confluence (l'adresse est-elle la bonne ?): {}", e))
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

/// Découpe un texte en passages d'environ `CHUNK_CHARS` caractères, en coupant
/// aux fins de paragraphe. Retourne des positions d'octets valides.
fn split_chunks(text: &str) -> Vec<(usize, usize)> {
    let mut paragraphs = Vec::new();
    let mut start = 0;
    let mut last_break = 0;

    for (i, _) in text.match_indices('\n') {
        if i - start >= CHUNK_CHARS && last_break > start {
            paragraphs.push((start, last_break));
            start = last_break;
        }
        last_break = i + 1;
    }
    paragraphs.push((start, text.len()));

    // Un paragraphe démesuré (tableau aplati en une ligne) est coupé net, sur
    // une frontière de caractère.
    let mut chunks = Vec::new();
    for (mut s, e) in paragraphs {
        while e - s > CHUNK_CHARS * 2 {
            let mut cut = s + CHUNK_CHARS;
            while !text.is_char_boundary(cut) {
                cut += 1;
            }
            chunks.push((s, cut));
            s = cut;
        }
        chunks.push((s, e));
    }
    chunks.retain(|(s, e)| !text[*s..*e].trim().is_empty());
    chunks
}

/// Convertit le format de stockage de Confluence (XHTML) en texte brut lisible
/// par le modèle : balises retirées, blocs séparés par des sauts de ligne,
/// entités décodées.
pub fn storage_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;

    while let Some(pos) = rest.find(|c| c == '<' || c == '&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];

        if rest.starts_with("<![CDATA[") {
            // Contenu des macros de code : gardé tel quel.
            let body = &rest[9..];
            let end = body.find("]]>").unwrap_or(body.len());
            out.push_str(&body[..end]);
            out.push('\n');
            rest = &body[(end + 3).min(body.len())..];
        } else if rest.starts_with('<') {
            let end = rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
            let tag = &rest[1..end.saturating_sub(1)];
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == ':' || *c == '-')
                .collect::<String>()
                .to_lowercase();
            match name.as_str() {
                "p" | "br" | "div" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                | "pre" | "blockquote" | "table" | "ul" | "ol" => out.push('\n'),
                "li" if !tag.starts_with('/') => out.push_str("\n- "),
                "td" | "th" if !tag.starts_with('/') => out.push_str(" | "),
                _ => {}
            }
            rest = &rest[end..];
        } else {
            // Entité : « &eacute; », « &#233; », « &#xE9; ».
            let end = rest[1..]
                .find(|c: char| c == ';' || c == '&' || c == '<' || c.is_whitespace())
                .map(|e| e + 1);
            match end {
                Some(e) if rest[e..].starts_with(';') && e <= 10 => {
                    match decode_entity(&rest[1..e]) {
                        Some(c) => out.push(c),
                        None => out.push_str(&rest[..=e]),
                    }
                    rest = &rest[e + 1..];
                }
                _ => {
                    out.push('&');
                    rest = &rest[1..];
                }
            }
        }
    }
    out.push_str(rest);
    normalize_whitespace(&out)
}

fn decode_entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            num.parse().ok()?
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "rsquo" | "lsquo" => '\'',
        "rdquo" | "ldquo" => '"',
        "laquo" => '«',
        "raquo" => '»',
        "ndash" => '–',
        "mdash" => '—',
        "hellip" => '…',
        "euro" => '€',
        "agrave" => 'à',
        "acirc" => 'â',
        "eacute" => 'é',
        "egrave" => 'è',
        "ecirc" => 'ê',
        "euml" => 'ë',
        "icirc" => 'î',
        "iuml" => 'ï',
        "ocirc" => 'ô',
        "ugrave" => 'ù',
        "ucirc" => 'û',
        "uuml" => 'ü',
        "ccedil" => 'ç',
        "Agrave" => 'À',
        "Eacute" => 'É',
        "Egrave" => 'È',
        "Ecirc" => 'Ê',
        "Ccedil" => 'Ç',
        "oelig" => 'œ',
        _ => return None,
    })
}

/// Espaces multiples réduits à un seul, lignes vides réduites à une seule.
fn normalize_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_lines = 0;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() || line == "-" || line == "|" {
            blank_lines += 1;
            if blank_lines == 1 && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        blank_lines = 0;
        out.push_str(&line);
        out.push('\n');
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_to_text_keeps_structure_and_entities() {
        let html = "<h1>Acc&egrave;s VPN</h1><p>Installez <strong>FortiClient</strong>&nbsp;:</p>\
<ul><li>Ouvrir</li><li>Se connecter</li></ul>\
<ac:structured-macro ac:name=\"code\"><ac:plain-text-body><![CDATA[vpn.exe --up]]></ac:plain-text-body></ac:structured-macro>";
        assert_eq!(
            storage_to_text(html),
            "Accès VPN\n\nInstallez FortiClient :\n\n- Ouvrir\n- Se connecter\nvpn.exe --up"
        );
    }

    #[test]
    fn split_chunks_covers_long_text_on_char_boundaries() {
        let text = "é".repeat(5000);
        let chunks = split_chunks(&text);
        assert!(chunks.len() > 1);
        assert_eq!(chunks.last().unwrap().1, text.len());
        for (s, e) in chunks {
            assert!(text.is_char_boundary(s) && text.is_char_boundary(e));
        }
    }
}
