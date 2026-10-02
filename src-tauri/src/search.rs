//! Recherche plein texte (BM25) sur des passages courts, sans dépendance ni
//! service externe.
//!
//! Des plongements vectoriels seraient plus fins, mais aucun des fournisseurs
//! configurables n'en offre un commun (Anthropic n'en propose pas, un serveur
//! local pas forcément). BM25 marche partout, hors ligne, et trouve très bien
//! une page Confluence qui emploie les mêmes mots que la question — ce qui est
//! le cas courant pour de la documentation interne.

use std::collections::HashMap;

/// Mots trop fréquents pour distinguer un passage d'un autre. Ils sont retirés
/// après le pliage des accents, d'où leur écriture sans accents.
const STOPWORDS: &[&str] = &[
    // français
    "au", "aux", "avec", "ce", "ces", "cet", "cette", "dans", "de", "des", "du", "elle", "en",
    "et", "eux", "il", "ils", "je", "la", "le", "les", "leur", "lui", "ma", "mais", "me", "meme",
    "mes", "moi", "mon", "ne", "nos", "notre", "nous", "on", "ou", "par", "pas", "pour", "qu",
    "que", "qui", "sa", "se", "ses", "son", "sur", "ta", "te", "tes", "toi", "ton", "tu", "un",
    "une", "vos", "votre", "vous", "est", "sont", "ete", "etre", "avoir", "ai", "as", "avez",
    "ont", "suis", "es", "sommes", "etes", "fait", "faire", "plus", "tres", "bien", "aussi",
    "comme", "si", "tout", "tous", "toute", "toutes", "donc", "alors", "cela", "ca", "ici",
    "bonjour", "salut", "merci", "cordialement", "svp",
    // anglais
    "the", "and", "or", "of", "to", "in", "on", "for", "with", "is", "are", "was", "were", "be",
    "been", "it", "this", "that", "these", "those", "an", "as", "at", "by", "from", "we", "you",
    "they", "he", "she", "our", "your", "their", "not", "no", "do", "does", "did", "can",
    "will", "would", "should", "could", "have", "has", "had", "hi", "hello", "thanks", "please",
];

/// Ramène un caractère accentué à sa lettre de base : « procédure » et
/// « procedure » doivent se retrouver, les accents étant souvent omis au clavier.
fn fold(c: char) -> char {
    match c {
        'à' | 'â' | 'ä' | 'á' | 'ã' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'î' | 'ï' | 'í' | 'ì' => 'i',
        'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
        'ù' | 'û' | 'ü' | 'ú' => 'u',
        'ç' => 'c',
        'ÿ' => 'y',
        'ñ' => 'n',
        other => other,
    }
}

/// Découpe un texte en termes comparables : minuscules, sans accents, sans
/// mots vides, avec un pluriel grossièrement retiré.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();

    let flush =|current: &mut String, terms: &mut Vec<String>| {
        if current.chars().count() >= 2 && !STOPWORDS.contains(&current.as_str()) {
            let mut term = std::mem::take(current);
            // « serveurs » / « serveur », « réseaux » / « réseau » : une
            // racinisation complète n'apporterait pas grand-chose de plus.
            if term.len() > 4 && (term.ends_with('s') || term.ends_with('x')) {
                term.pop();
            }
            terms.push(term);
        }
        current.clear();
    };

    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            current.push(fold(c));
        } else {
            flush(&mut current, &mut terms);
        }
    }
    flush(&mut current, &mut terms);
    terms
}

const K1: f64 = 1.2;
const B: f64 = 0.75;

/// Index BM25 en mémoire. Les documents sont désignés par leur rang
/// d'insertion.
pub struct Bm25 {
    docs: Vec<HashMap<String, u32>>,
    lengths: Vec<u32>,
    doc_freq: HashMap<String, u32>,
    avg_len: f64,
}

impl Bm25 {
    pub fn new<I>(documents: I) -> Self
    where
        I: IntoIterator<Item = Vec<String>>,
    {
        let mut docs = Vec::new();
        let mut lengths = Vec::new();
        let mut doc_freq: HashMap<String, u32> = HashMap::new();

        for terms in documents {
            let mut freq: HashMap<String, u32> = HashMap::new();
            for term in &terms {
                *freq.entry(term.clone()).or_default() += 1;
            }
            for term in freq.keys() {
                *doc_freq.entry(term.clone()).or_default() += 1;
            }
            lengths.push(terms.len() as u32);
            docs.push(freq);
        }

        let total: u64 = lengths.iter().map(|&l| l as u64).sum();
        let avg_len = if docs.is_empty() {
            1.0
        } else {
            (total as f64 / docs.len() as f64).max(1.0)
        };

        Bm25 {
            docs,
            lengths,
            doc_freq,
            avg_len,
        }
    }

    /// Les `limit` meilleurs documents pour la requête, du plus pertinent au
    /// moins pertinent. Un document sans aucun terme commun n'est jamais
    /// retourné, même s'il en manque pour atteindre `limit`.
    pub fn search(&self, query: &[String], limit: usize) -> Vec<(usize, f64)> {
        let n = self.docs.len() as f64;
        let mut unique: Vec<&String> = query.iter().collect();
        unique.sort();
        unique.dedup();

        // Poids de chaque terme de la requête, calculé une fois.
        let weights: Vec<(&String, f64)> = unique
            .into_iter()
            .filter_map(|term| {
                let df = *self.doc_freq.get(term)? as f64;
                Some((term, ((n - df + 0.5) / (df + 0.5) + 1.0).ln()))
            })
            .collect();

        let mut scored: Vec<(usize, f64)> = self
            .docs
            .iter()
            .enumerate()
            .filter_map(|(i, freq)| {
                let norm = K1 * (1.0 - B + B * self.lengths[i] as f64 / self.avg_len);
                let score: f64 = weights
                    .iter()
                    .filter_map(|(term, idf)| {
                        let tf = *freq.get(*term)? as f64;
                        Some(idf * tf * (K1 + 1.0) / (tf + norm))
                    })
                    .sum();
                (score > 0.0).then_some((i, score))
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_folds_accents_and_plurals() {
        assert_eq!(
            tokenize("Les Procédures VPN, réseaux"),
            vec!["procedure", "vpn", "reseau"]
        );
    }

    #[test]
    fn search_prefers_matching_document() {
        let index = Bm25::new(vec![
            tokenize("Configurer le VPN sur un poste Windows"),
            tokenize("Commander un nouvel écran"),
            tokenize("Politique de congés"),
        ]);
        let hits = index.search(&tokenize("comment configurer le vpn ?"), 3);
        assert_eq!(hits.first().map(|h| h.0), Some(0));
        assert_eq!(hits.len(), 1);
    }
}
