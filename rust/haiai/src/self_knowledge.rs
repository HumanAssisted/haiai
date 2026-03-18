//! Embedded documentation search powered by BM25.
//!
//! All jacsbook chapters and SDK READMEs are compiled into the binary as static
//! strings. On first query a [`SearchEngine`] is built in memory via [`OnceLock`]
//! and reused for all subsequent calls.
//!
//! ```rust
//! use haiai::self_knowledge::{self_knowledge, KnowledgeResult};
//!
//! let results: Vec<KnowledgeResult> = self_knowledge("key rotation", 5);
//! for r in &results {
//!     println!("[{}] {} (score: {:.2})", r.rank, r.title, r.score);
//! }
//! ```

use bm25::{Document, Language, LanguageMode, SearchEngine, SearchEngineBuilder};
use std::sync::OnceLock;

#[path = "self_knowledge_data.rs"]
mod self_knowledge_data;
use self_knowledge_data::CHAPTERS;

static ENGINE: OnceLock<SearchEngine<u32>> = OnceLock::new();

/// A single search result from the embedded documentation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KnowledgeResult {
    /// 1-based rank in the result set.
    pub rank: usize,
    /// Relative path of the source document (e.g. `"jacsbook/advanced/key-rotation.md"`).
    pub path: String,
    /// Human-readable title extracted from SUMMARY.md.
    pub title: String,
    /// Full markdown content of the chapter.
    pub content: String,
    /// BM25 relevance score.
    pub score: f32,
}

fn get_engine() -> &'static SearchEngine<u32> {
    ENGINE.get_or_init(|| {
        let docs: Vec<Document<u32>> = CHAPTERS
            .iter()
            .enumerate()
            .map(|(i, &(_, _, content))| Document::new(i as u32, content.to_string()))
            .collect();
        SearchEngineBuilder::with_documents(LanguageMode::Fixed(Language::English), docs).build()
    })
}

/// Search the embedded JACS and HAI documentation.
///
/// Returns up to `limit` results ranked by BM25 relevance. An empty query
/// returns an empty vec.
pub fn self_knowledge(query: &str, limit: usize) -> Vec<KnowledgeResult> {
    if query.is_empty() {
        return vec![];
    }
    let engine = get_engine();
    engine
        .search(query, limit)
        .into_iter()
        .enumerate()
        .map(|(rank, result)| {
            let idx = result.document.id as usize;
            let &(path, title, _) = &CHAPTERS[idx];
            KnowledgeResult {
                rank: rank + 1,
                path: path.to_string(),
                title: title.to_string(),
                content: result.document.contents.clone(),
                score: result.score,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jacs_query_returns_results() {
        let results = self_knowledge("JACS", 5);
        assert!(!results.is_empty(), "JACS query should return results");
    }

    #[test]
    fn empty_query_returns_empty() {
        let results = self_knowledge("", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn limit_is_respected() {
        let results = self_knowledge("key rotation", 1);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn results_sorted_by_score_descending() {
        let results = self_knowledge("document signing", 5);
        for w in results.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
    }

    #[test]
    fn results_have_nonempty_fields() {
        let results = self_knowledge("JACS", 5);
        for r in &results {
            assert!(!r.path.is_empty());
            assert!(!r.title.is_empty());
            assert!(!r.content.is_empty());
        }
    }

    #[test]
    fn scores_are_positive() {
        let results = self_knowledge("JACS", 5);
        for r in &results {
            assert!(r.score > 0.0);
        }
    }
}
