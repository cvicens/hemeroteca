/// Module for relevance related functions
use once_cell::sync::Lazy;

use std::collections::HashSet;
use strsim::sorensen_dice;

use crate::common::{NewsItem, DEFAULT_CONFIG_FOLDER_NAME, DEFAULT_ROOT_WORDS_FILE};

const DICE_COEFFICIENT: f64 = 0.75;

#[rustfmt::skip]
static ROOT_WORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "Presidente", "Presidencial", "Gobierno", "Crisis", "Elección", "Elecciones", "Ley", "Ministro", "Economía", "Defensa",
        "Inflación", "Desempleo", "Reforma", "Diplomático", "Crisis", "Ataque", "Seguridad", "Migración",
        "Infección", "Hospital", "Tecnología", "Tecnológico", "Innovación", "Ciberseguridad", "Clima", "Energía",
        "Guerra", "Conflicto", "Policía", "Crimen", "Corrupción", "Arresto", "Caos", "Protesta",
        "President", "Presidential", "Government", "Crisis", "Election", "Law", "Minister", "Economy", "Defense",
        "Inflation", "Unemployment", "Reform", "Diplomatic", "Crisis", "Attack", "Security", "Migration",
        "Infection", "Hospital", "Technology", "Technologic", "Innovation", "Cybersecurity", "Climate", "Energy",
        "War", "Conflict", "Police", "Crime", "Corruption", "Arrest", "Chaos", "Protest",
    ]
    .iter()
    .cloned()
    .collect()
});

/// Struct to represent the relevance of a NewsItem
#[derive(Debug, PartialEq, Clone)]
pub struct Relevance {
    pub error: bool,
    pub relevance_core: u64,
    pub relevance_content: u64,
    pub explanation: String,
    pub elapsed_time: f64,
}

impl Relevance {
    pub fn new(relevance_core: (bool, u64, u64, u64, u64, u64), relevance_content: u64, elapsed_time: f64) -> Self {
        Self {
            error: relevance_core.0,
            relevance_core: relevance_core.1 + relevance_core.2 + relevance_core.3 + relevance_core.4 + relevance_core.5,
            relevance_content,
            explanation: Relevance::build_explanation(relevance_core.0, relevance_core.1, relevance_core.2, relevance_core.3, relevance_core.4, relevance_core.5),
            elapsed_time,
        }
    }

    pub fn to_string(&self) -> String {
        format!(
            "Relevance[core: {}, content: {}, explanation: '{}']",
            self.relevance_core, self.relevance_content, self.explanation
        )
    }

    fn build_explanation(error: bool, by_creator: u64, by_categories: u64, by_keyword: u64, by_title: u64, by_content: u64) -> String {
        if error {
            "Error in the news item".to_string();
        }
        format!(
            "breakdown [creator: {}, categories: {}, keywords: {}, title: {}, content: {}]",
            by_creator, by_categories, by_keyword, by_title, by_content
        )
    }

    // Function that returns the net relevance of a NewsItem
    pub fn net_relevance(&self) -> u64 {
        self.relevance_core + self.relevance_content
    }

    // cmp implementation for Relevance comparing by error, relevance_core adn relevance_content
    pub fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.error != other.error {
            return self.error.cmp(&other.error);
        }
        if self.relevance_core != other.relevance_core {
            return self.relevance_core.cmp(&other.relevance_core);
        }
        self.relevance_content.cmp(&other.relevance_content)
    }
    
}

/// Function to get additional words from file ~/.hemeroteca/root_words.txt and add them to the ROOT_WORDS HashSet returning a new HashSet 
fn get_combined_root_words() -> HashSet<String> {
    let mut additional_words = HashSet::new();
    let home_dir = dirs::home_dir().unwrap();
    
    let root_words_file = home_dir.join(DEFAULT_CONFIG_FOLDER_NAME).join(DEFAULT_ROOT_WORDS_FILE);
    if let Ok(file) = std::fs::read_to_string(root_words_file) {
        for word in file.split_whitespace() {
            additional_words.insert(word.to_string());
        }
    }

    additional_words.extend(ROOT_WORDS.iter().cloned().map(String::from));

    additional_words
}

// Function to check if any root word is within a certain Levenshtein distance
fn similar_to_root_word(word: &str, coefficient: f64) -> bool {
    let root_words = get_combined_root_words();
    for root in root_words {
        if sorensen_dice(&root, word) >= coefficient {
            log::trace!("Root word '{}' is similar to '{}'", root, word);
            return true;
        } else {
            log::trace!("Root word '{}' is not similar to '{}'", root, word);
        }
    }
    false
}

/// Function that calculates the relevance_core of a NewsItem
fn calculate_relevance_core(news_item: &NewsItem) -> (bool, u64, u64, u64, u64, u64) {
    // If the new item has an error, return 0 relevance
    if news_item.error.is_some() {
        return (true, 0, 0, 0, 0, 0);
    }

    // If the news item has a creator not empty, increase the relevance
    let relevance_by_creator = if news_item.creators.len() > 0 {
        10
    } else {
        0
    };

    // If any of the categories is similar to a root word, increase the relevance by 1 for each
    let relevance_by_categories = if let Some(categories) = &news_item.categories {
        let mut relevance = 0;
        for category in categories.split(",") {
            if similar_to_root_word(category, DICE_COEFFICIENT) {
                relevance += 5;
            }
        }
        relevance
    } else {
        0
    };

    // If any of the keywords is similar to a root word, increase the relevance by 1 for each
    let relevance_by_keywords = if let Some(keywords) = &news_item.keywords {
        let mut relevance = 0;
        for keyword in keywords.split(",") {
            if similar_to_root_word(keyword, DICE_COEFFICIENT) {
                relevance += 5;
            }
        }
        relevance
    } else {
        0
    };

    // f any of the word in the title is similar to a root word, increase the relevance by 1 for each
    let relevance_by_title = if news_item.title.len() > 0 {
        let mut relevance = 0;
        for word in news_item.title.split_whitespace() {
            if similar_to_root_word(word, DICE_COEFFICIENT) {
                relevance += 10;
            }
        }
        relevance
    } else {
        0
    };

    // If any of the word in the description is similar to a root word, increase the relevance by 1 for each
    let relevance_by_description = if news_item.description.len() > 0 {
        let mut relevance = 0;
        for word in news_item.description.split_whitespace() {
            if similar_to_root_word(word, DICE_COEFFICIENT) {
                relevance += 1;
            }
        }
        relevance
    } else {
        0
    };

    (false, relevance_by_creator, relevance_by_categories, relevance_by_keywords, relevance_by_title, relevance_by_description)
}

/// Function that calculates the relevance_full of a NewsItem
pub async fn calculate_relevance(news_item: &NewsItem) -> Relevance {
    // Start time
    let start = std::time::Instant::now();

    // Calculate the relevance_core of the news item
    let relevance_core = calculate_relevance_core(news_item);

    // If the news item has an error, return 0 relevance
    let relevance = if relevance_core.0 {
        Relevance::new(relevance_core, 0, 0.0)
    } else {
        // If the news item has a clean content, increase the relevance
        let relevance_content = if let Some(clean_content) = &news_item.clean_content {
            let mut relevance = 0;
            for word in clean_content.split_whitespace() {
                if similar_to_root_word(word, DICE_COEFFICIENT) {
                    relevance += 1;
                }
            }
            relevance
        } else {
            0
        };

        let elapsed_time = start.elapsed().as_secs_f64();
        Relevance::new(relevance_core, relevance_content, elapsed_time)
    };

    relevance
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{NewsItem, PipelineError};
    
    #[test]
    fn test_similar_to_root_word() {
        assert_eq!(similar_to_root_word("Presidente", DICE_COEFFICIENT), true);
        assert_eq!(similar_to_root_word("President", DICE_COEFFICIENT), true);
        assert_eq!(similar_to_root_word("Presidencial", DICE_COEFFICIENT), true);
        assert_eq!(similar_to_root_word("Presidential", DICE_COEFFICIENT), true);
        assert_eq!(similar_to_root_word("Elección", DICE_COEFFICIENT), true);
        assert_eq!(similar_to_root_word("Elecciones", DICE_COEFFICIENT), true);
        assert_eq!(similar_to_root_word("Election", DICE_COEFFICIENT), true);
        assert_eq!(similar_to_root_word("Elections", DICE_COEFFICIENT), true);
        assert_eq!(similar_to_root_word("Clima", DICE_COEFFICIENT), true);
        assert_eq!(similar_to_root_word("Climate", DICE_COEFFICIENT), true);
        assert_eq!(similar_to_root_word("Climático", DICE_COEFFICIENT), false);
        assert_eq!(similar_to_root_word("Technology", DICE_COEFFICIENT), true);
        assert_eq!(similar_to_root_word("Tecnológico", DICE_COEFFICIENT), true);
    }
    
    #[test]
    fn test_calculate_relevance_core() {
        let news_item_with_error = NewsItem {
            error: Some(PipelineError::EmptyString),
            creators: "".to_string(),
            categories: None,
            keywords: None,
            title: "".to_string(),
            description: "".to_string(),
            clean_content: None,
            channel: "".to_string(),
            link: "".to_string(),
            pub_date: None,
            relevance: None,
        };
        assert_eq!(calculate_relevance_core(&news_item_with_error), (true, 0, 0, 0, 0, 0));
        
        let news_item_with_creators = NewsItem {
            error: None,
            creators: "John Doe".to_string(),
            categories: None,
            keywords: None,
            title: "".to_string(),
            description: "".to_string(),
            clean_content: None,
            channel: "".to_string(),
            link: "".to_string(),
            pub_date: None,
            relevance: None,
        };
        assert_eq!(calculate_relevance_core(&news_item_with_creators), (false, 10, 0, 0, 0, 0));
        
        let news_item_with_categories = NewsItem {
            error: None,
            creators: "".to_string(),
            categories: Some("Politics, Economy, Technology".to_string()),
            keywords: None,
            title: "".to_string(),
            description: "".to_string(),
            clean_content: None,
            channel: "".to_string(),
            link: "".to_string(),
            pub_date: None,
            relevance: None,
        };
        assert_eq!(calculate_relevance_core(&news_item_with_categories), (false, 0, 10, 0, 0, 0));
        
        let news_item_with_keywords = NewsItem {
            error: None,
            creators: "".to_string(),
            categories: None,
            keywords: Some("Inflation, Climate, Security".to_string()),
            title: "".to_string(),
            description: "".to_string(),
            clean_content: None,
            channel: "".to_string(),
            link: "".to_string(),
            pub_date: None,
            relevance: None,
        };
        assert_eq!(calculate_relevance_core(&news_item_with_keywords), (false, 0, 0, 15, 0, 0));
        
        let news_item_with_title = NewsItem {
            error: None,
            creators: "".to_string(),
            categories: None,
            keywords: None,
            title: "Presidente Elections".to_string(),
            description: "".to_string(),
            clean_content: None,
            channel: "".to_string(),
            link: "".to_string(),
            pub_date: None,
            relevance: None,
        };
        assert_eq!(calculate_relevance_core(&news_item_with_title), (false, 0, 0, 0, 20, 0));
        
        let news_item_with_description = NewsItem {
            error: None,
            creators: "".to_string(),
            categories: None,
            keywords: None,
            title: "".to_string(),
            description: "Crisis and Security".to_string(),
            clean_content: None,
            channel: "".to_string(),
            link: "".to_string(),
            pub_date: None,
            relevance: None,
        };
        assert_eq!(calculate_relevance_core(&news_item_with_description), (false, 0, 0, 0, 0, 2));

        let news_item_with_description = NewsItem {
            error: None,
            creators: "John Doe".to_string(),
            categories: None,
            keywords: None,
            title: "".to_string(),
            description: "Crisis and Security".to_string(),
            clean_content: None,
            channel: "".to_string(),
            link: "".to_string(),
            pub_date: None,
            relevance: None,
        };
        assert_eq!(calculate_relevance_core(&news_item_with_description), (false, 10, 0, 0, 0, 2));
    }
    
    #[tokio::test]
    async fn test_calculate_relevance() {
        let news_item_with_clean_content = NewsItem {
            error: None,
            creators: "".to_string(),
            categories: None,
            keywords: None,
            title: "".to_string(),
            description: "".to_string(),
            clean_content: Some("Presidente Elections Crisis Modernización".to_string()),
            channel: "".to_string(),
            link: "".to_string(),
            pub_date: None,
            relevance: None,
        };
        
        let relevance_core = calculate_relevance_core(&news_item_with_clean_content);
        let relevance = calculate_relevance(&news_item_with_clean_content).await;
        let relevance_content = relevance.relevance_content;

        assert_eq!(relevance, Relevance::new(relevance_core, relevance_content, relevance.elapsed_time));
        
        let news_item_without_clean_content = NewsItem {
            error: None,
            creators: "".to_string(),
            categories: None,
            keywords: None,
            title: "".to_string(),
            description: "".to_string(),
            clean_content: None,
            channel: "".to_string(),
            link: "".to_string(),
            pub_date: None,
            relevance: None,
        };

        let relevance_core = calculate_relevance_core(&news_item_without_clean_content);
        let relevance = calculate_relevance(&news_item_without_clean_content).await;
        let relevance_content = relevance.relevance_content;

        assert_eq!(relevance, Relevance::new(relevance_core, relevance_content, relevance.elapsed_time));
    }
}
