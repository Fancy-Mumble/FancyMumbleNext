//! Persistent cert-hash to username resolver with Docker-style fallback names.
//!
//! Provides a trait-based abstraction so the resolution strategy can be
//! swapped out.  The default implementation persists known mappings to a
//! JSON file and generates deterministic human-readable names for unknown
//! hashes using adjective + animal word lists (similar to Docker's
//! `names-generator`).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use tracing::warn;

use crate::utils::hex_to_bytes;

// --- Word lists (inspired by Docker's names-generator) ---

const ADJECTIVES: &[&str] = &[
    "admiring", "adoring", "agitated", "amazing", "angry",
    "beautiful", "bold", "brave", "busy", "calm",
    "charming", "clever", "cool", "crazy", "dazzling",
    "determined", "dreamy", "eager", "ecstatic", "elastic",
    "elated", "elegant", "epic", "exciting", "fervent",
    "festive", "flamboyant", "focused", "friendly", "frosty",
    "gallant", "gifted", "goofy", "gracious", "happy",
    "hopeful", "hungry", "infallible", "inspiring", "intelligent",
    "interesting", "jolly", "keen", "kind", "laughing",
    "loving", "lucid", "magical", "modest", "musing",
    "mystifying", "naughty", "nervous", "nice", "nifty",
    "nostalgic", "objective", "optimistic", "peaceful", "pedantic",
    "pensive", "practical", "priceless", "quirky", "quizzical",
    "recursing", "relaxed", "reverent", "romantic", "sad",
    "serene", "sharp", "silly", "sleepy", "stoic",
    "strange", "stupefied", "suspicious", "sweet", "tender",
    "thirsty", "trusting", "unruffled", "upbeat", "vibrant",
    "vigilant", "vigorous", "wizardly", "wonderful", "youthful",
    "zealous", "zen", "adaptable", "affectionate", "adventurous",
    "bright", "cheerful", "curious", "gentle", "radiant",
];

const ANIMALS: &[&str] = &[
    "albatross", "alpaca", "badger", "bear", "butterfly",
    "cardinal", "chameleon", "cheetah", "crane", "deer",
    "dolphin", "eagle", "elephant", "falcon", "flamingo",
    "fox", "gazelle", "giraffe", "hawk", "hedgehog",
    "heron", "iguana", "impala", "jackal", "jaguar",
    "kangaroo", "kingfisher", "koala", "lemur", "leopard",
    "lion", "lynx", "macaw", "meerkat", "moose",
    "narwhal", "newt", "nightingale", "octopus", "osprey",
    "otter", "owl", "panda", "pangolin", "parrot",
    "pelican", "penguin", "quail", "quetzal", "raccoon",
    "raven", "robin", "salamander", "seal", "sparrow",
    "starling", "swan", "tiger", "toucan", "turtle",
    "viper", "walrus", "whale", "wolf", "wren",
    "zebra", "antelope", "armadillo", "bison", "buffalo",
    "camel", "capybara", "cougar", "coyote", "crow",
    "dingo", "ferret", "finch", "gecko", "gorilla",
    "hamster", "hyena", "kestrel", "lizard", "llama",
    "mantis", "marmot", "mink", "mole", "panther",
    "peacock", "pheasant", "porpoise", "rabbit", "reindeer",
    "rhino", "shrew", "sloth", "tapir", "weasel",
];

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Resolves a certificate hash to a human-readable display name.
pub trait HashNameResolver: Send + Sync {
    /// Look up or generate a display name for the given `cert_hash`.
    ///
    /// Returns the stored username when available, otherwise a deterministic
    /// human-readable name derived from the hash.
    fn resolve(&self, cert_hash: &str) -> String;

    /// Record that `username` is associated with `cert_hash`.
    /// Persists the mapping so future calls to [`resolve`] return this name.
    fn record(&self, cert_hash: &str, username: &str);

    /// Generate a deterministic human-readable fallback name from a hex-encoded
    /// hash string.
    ///
    /// Uses two bytes of the hash to select an adjective and an animal,
    /// producing names like "Brave Falcon" or "Calm Otter".
    /// Override to customise the fallback naming strategy.
    fn generate_fallback_name(&self, cert_hash: &str) -> String {
        let bytes = hex_to_bytes(cert_hash);

        let adj_idx = if bytes.is_empty() {
            0
        } else {
            usize::from(bytes[0])
        };
        let noun_idx = if bytes.len() < 2 {
            0
        } else {
            usize::from(bytes[1])
        };

        let adj = ADJECTIVES[adj_idx % ADJECTIVES.len()];
        let noun = ANIMALS[noun_idx % ANIMALS.len()];
        format!("{adj} {noun}")
    }
}

// ---------------------------------------------------------------------------
// Default implementation
// ---------------------------------------------------------------------------

pub struct DefaultHashNameResolver {
    mappings: Mutex<HashMap<String, String>>,
    storage_path: PathBuf,
}

impl DefaultHashNameResolver {
    /// Create a new resolver that persists mappings to `storage_path`.
    ///
    /// Loads any existing mappings from the file on construction.
    pub fn new(storage_path: PathBuf) -> Self {
        let mappings = Self::load_from_file(&storage_path);
        Self {
            mappings: Mutex::new(mappings),
            storage_path,
        }
    }

    fn load_from_file(path: &PathBuf) -> HashMap<String, String> {
        match fs::read_to_string(path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    fn save_to_file(path: &PathBuf, mappings: &HashMap<String, String>) {
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                warn!("failed to create hash_names directory: {e}");
                return;
            }
        }
        match serde_json::to_string_pretty(mappings) {
            Ok(json) => {
                if let Err(e) = fs::write(path, json) {
                    warn!("failed to write hash_names file: {e}");
                }
            }
            Err(e) => warn!("failed to serialize hash_names: {e}"),
        }
    }
}

impl HashNameResolver for DefaultHashNameResolver {
    fn resolve(&self, cert_hash: &str) -> String {
        let guard = self.mappings.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(name) = guard.get(cert_hash) {
            return name.clone();
        }
        self.generate_fallback_name(cert_hash)
    }

    fn record(&self, cert_hash: &str, username: &str) {
        if cert_hash.is_empty() || username.is_empty() {
            return;
        }
        let mut guard = self.mappings.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let existing = guard.get(cert_hash);
        if existing.is_some_and(|n| n == username) {
            return;
        }
        guard.insert(cert_hash.to_owned(), username.to_owned());
        Self::save_to_file(&self.storage_path, &guard);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn deterministic_name_generation() {
        let resolver = DefaultHashNameResolver::new(
            NamedTempFile::new().unwrap().path().to_path_buf(),
        );
        let name1 = resolver.generate_fallback_name("abcdef1234567890");
        let name2 = resolver.generate_fallback_name("abcdef1234567890");
        assert_eq!(name1, name2, "same hash must produce same name");
    }

    #[test]
    fn different_hashes_different_names() {
        let resolver = DefaultHashNameResolver::new(
            NamedTempFile::new().unwrap().path().to_path_buf(),
        );
        let name1 = resolver.generate_fallback_name("0011223344556677");
        let name2 = resolver.generate_fallback_name("ff11223344556677");
        assert_ne!(name1, name2, "different first byte should give different adjective");
    }

    #[test]
    fn name_is_two_words() {
        let resolver = DefaultHashNameResolver::new(
            NamedTempFile::new().unwrap().path().to_path_buf(),
        );
        let name = resolver.generate_fallback_name("deadbeef");
        let parts: Vec<&str> = name.split(' ').collect();
        assert_eq!(parts.len(), 2, "name should be 'Adjective Animal'");
    }

    #[test]
    fn empty_hash_does_not_panic() {
        let resolver = DefaultHashNameResolver::new(
            NamedTempFile::new().unwrap().path().to_path_buf(),
        );
        let name = resolver.generate_fallback_name("");
        assert!(!name.is_empty());
    }

    #[test]
    fn resolve_returns_generated_name_for_unknown_hash() {
        let tmp = NamedTempFile::new().unwrap();
        let resolver = DefaultHashNameResolver::new(tmp.path().to_path_buf());
        let name = resolver.resolve("deadbeef");
        assert_eq!(name, resolver.generate_fallback_name("deadbeef"));
    }

    #[test]
    fn record_and_resolve_returns_stored_name() {
        let tmp = NamedTempFile::new().unwrap();
        let resolver = DefaultHashNameResolver::new(tmp.path().to_path_buf());
        resolver.record("abcdef", "Alice");
        assert_eq!(resolver.resolve("abcdef"), "Alice");
    }

    #[test]
    fn persists_across_instances() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        {
            let resolver = DefaultHashNameResolver::new(path.clone());
            resolver.record("abc123", "Bob");
        }
        let resolver2 = DefaultHashNameResolver::new(path);
        assert_eq!(resolver2.resolve("abc123"), "Bob");
    }

    #[test]
    fn record_ignores_empty_values() {
        let tmp = NamedTempFile::new().unwrap();
        let resolver = DefaultHashNameResolver::new(tmp.path().to_path_buf());
        resolver.record("", "Alice");
        resolver.record("abc", "");
        // Neither should be stored; both should resolve to generated names
        let name = resolver.resolve("abc");
        assert_eq!(name, resolver.generate_fallback_name("abc"));
    }

    #[test]
    fn loads_existing_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, r#"{{"aabbcc":"Charlie"}}"#).unwrap();
        tmp.flush().unwrap();
        let resolver = DefaultHashNameResolver::new(tmp.path().to_path_buf());
        assert_eq!(resolver.resolve("aabbcc"), "Charlie");
    }

    #[test]
    fn handles_corrupt_file_gracefully() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "not valid json!!!").unwrap();
        tmp.flush().unwrap();
        let resolver = DefaultHashNameResolver::new(tmp.path().to_path_buf());
        // Should fall back to empty map; resolve produces generated name.
        let name = resolver.resolve("deadbeef");
        assert!(!name.is_empty());
    }
}
