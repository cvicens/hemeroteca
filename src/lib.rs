//! Library that provides functions to read and parse RSS feeds

pub mod common;
pub mod openai;
pub mod relevance;
pub mod storage;

// Re-export commonly used items in a prelude module
pub mod prelude {
    pub use crate::clean_content;
    pub use crate::common::ChannelType;
    pub use crate::common::NewsItem;
    pub use crate::common::Operator;
    pub use crate::common::PipelineError;
    pub use crate::fetch_news_items_opted_in;
    pub use crate::fill_news_item_content;
    pub use crate::fill_news_items_with_clean_contents;
    pub use crate::get_channel_type;
    pub use crate::insert_news_items;
    pub use crate::log_news_items_to_file;
    pub use crate::log_news_items_to_db;
    pub use crate::generate_dossier;
    pub use crate::openai::summarize;
    pub use crate::read_feed;
    pub use crate::read_urls;
    pub use crate::relevance::calculate_relevance;
    pub use crate::top_k_news_items;
    pub use crate::update_news_items_with_relevance;
    pub use crate::update_news_items_with_relevance_top_k;
}

use crate::relevance::calculate_relevance;
use common::{ChannelType, NewsItem, Operator, PipelineError};

use std::{
    error::Error,
    io::{BufRead, Cursor, Write},
};

use select::predicate::Name;
use select::{document::Document, node};

use html2text::config;
use rand::seq::SliceRandom;
use regex::Regex;
use rss::{Channel, Item};

/// Function that reads a feed from a URL
pub async fn read_feed(feed_url: &str) -> Result<Channel, Box<dyn Error>> {
    let content = reqwest::get(feed_url).await?.bytes().await?;
    let channel = Channel::read_from(&content[..])?;
    Ok(channel)
}

/// Function that reads feed urls from a file
///
/// Example:
/// ```
/// use hemeroteca::read_urls;
///
/// let urls = read_urls("feeds.txt").unwrap();
/// let count_ok = urls.len() >= 0;
/// assert_eq!(count_ok, true);
/// ```
pub fn read_urls(file: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let file = std::fs::File::open(file)?;
    let reader = std::io::BufReader::new(file);

    let url_regex = Regex::new(r#"(http|https)://[^\s/$.?#].[^\s]*"#)?;

    let urls: Vec<String> = reader
        .lines()
        .filter_map(|line| match line {
            Ok(line) => {
                if !line.is_empty() && !line.starts_with('#') && url_regex.is_match(&line) {
                    log::trace!("Accepting line: {}", line);
                    Some(line)
                } else {
                    log::warn!("Ignoring line: {}", line);
                    None
                }
            }
            Err(err) => {
                log::error!("Error reading line: {:?}", err);
                None
            }
        })
        .collect();
    Ok(urls)
}

/// Function that returns NewsItems from a vector of feed urls matching the
/// categories or keywords passed as a reference
pub async fn fetch_news_items_opted_in(
    feed_urls: &Vec<String>,
    opt_in: &Vec<String>,
    operator: Operator,
) -> Option<Vec<NewsItem>> {
    let mut channels = Vec::new();

    // Spawn as many thread as the minimum of max number of threads and the number
    // of urls and get the handles
    // log::trace!("Spawning {} tasks", feed_urls.len());
    let mut handles = vec![];
    for url in feed_urls.iter() {
        let url = url.clone();
        let handle = tokio::spawn(async move {
            let channel = read_feed(&url).await;
            if channel.is_err() {
                log::error!(
                    "Could not read the feed from {}. ERROR: {}",
                    url,
                    channel.err().unwrap()
                );
                None
            } else {
                Some(channel.unwrap())
            }
        });
        handles.push(handle);
    }

    // Wait for all the threads to finish
    for handle in handles {
        channels.push(handle.await.unwrap());
    }

    // Get the items from the channels
    let items: Vec<(&str, Vec<Item>)> = channels
        .iter()
        .filter_map(|channel| {
            if let Some(channel) = channel {
                // (Some(channel.title), Some(channel.items().to_vec()))
                Some((channel.title(), channel.items().to_vec()))
            } else {
                // (None, None)
                None
            }
        })
        .collect();

    // If there are no items return None
    if items.is_empty() {
        None
    } else {
        // Else, get all the items from the channels
        let mut all_items: Vec<NewsItem> = items
            .iter()
            .map(|(channel, items)| {
                items
                    .iter()
                    .map(|item| NewsItem::from_item(channel, item))
                    .filter_map(|result| {
                        if result.is_ok() {
                            Some(result.unwrap())
                        } else {
                            log::error!("Could not get the item from the result: {:?}", result);
                            None
                        }
                    })
                    .collect::<Vec<NewsItem>>()
            })
            .flatten()
            .collect();

        // Retains the items that have any of the categories or keywords equal to the
        // categories to opt in
        all_items.retain(|item| {
            let categories = item.categories.clone().unwrap_or("".to_string());
            let keywords = item.keywords.clone().unwrap_or("".to_string());
            log::trace!("Checking {:?} in {:?} and {:?}", opt_in, categories, keywords);

            // If the opt_in is empty, return true
            if opt_in.is_empty() {
                true
            } else {
                // Return true if all the opt_in items are in the categories or keywords
                match operator {
                    Operator::AND => opt_in
                        .iter()
                        .all(|item| categories.contains(item) || keywords.contains(item)),
                    Operator::OR => opt_in
                        .iter()
                        .any(|item| categories.contains(item) || keywords.contains(item)),
                }
            }
        });

        // Shuffle the items
        all_items.shuffle(&mut rand::thread_rng());

        Some(all_items)
    }
}

// /// Function that using rqwest gets all the contents of all the urls of a vec
// of NewsItems passed as a reference pub async fn get_all_contents(news_items:
// &Vec<NewsItem>) {     let mut contents = Vec::new();
//     for news_item in news_items {
//         let response = reqwest::get(&news_item.link).await?;
//         let content = response.text().await;
//         if let Ok(content) = content {
//             news_item.clean_content = html_to_text(content);
//         } else {
//             let error = content.err().unwrap().to_string();
//             log::error!(
//                 "Could not get the content from {}. ERROR: {}",
//                 news_item.link,
//                 error
//             );
//             news_item.clean_content =
// Err(PipelineError::NetworkError(error));         }
//     }
// }

/// Function that returns the channel given the channel name as a string
///
/// Example:
/// ```
/// use hemeroteca::prelude::*;
///
/// let channel = "EL PAÍS: el periódico global".to_string();
/// let channel_type = get_channel_type(&channel);
/// assert_eq!(channel_type, ChannelType::ElPais);
/// let channel = "20MINUTOS - ...".to_string();
/// let channel_type = get_channel_type(&channel);
/// assert_eq!(channel_type, ChannelType::VeinteMinutos);
/// let channel = "ElDiario.es".to_string();
/// let channel_type = get_channel_type(&channel);
/// assert_eq!(channel_type, ChannelType::ElDiario);
/// let channel = "Other".to_string();
/// let channel_type = get_channel_type(&channel);
/// assert_eq!(channel_type, ChannelType::Other);
/// ```
pub fn get_channel_type(channel: &String) -> ChannelType {
    // If channel in uppercase starts with "EL PAÍS" return ElPais
    if channel.to_uppercase().contains("EL PAÍS") {
        ChannelType::ElPais
    // If channel in uppercase starts with "20 MINUTOS" return VeinteMinutos
    } else if channel.to_uppercase().contains("20MINUTOS") {
        ChannelType::VeinteMinutos
    // If channel in uppercase starts with "EL DIARIO" return ElDiario
    } else if channel.to_uppercase().contains("ELDIARIO.ES") {
        ChannelType::ElDiario
    // If channel in uppercase starts with "ELMUNDO" return ElMundo
    } else if channel.to_uppercase().contains("ELMUNDO") {
        ChannelType::ElMundo
    // Otherwise return Other
    } else {
        ChannelType::Other
    }
}

/// Function that cleans the content of an html string depending on the feed it
/// comes from
///
/// Example:
///
/// ```
/// use hemeroteca::clean_content;
///
/// let channel = "Other".to_string();
/// let content = r#"
/// <html>
///    <head><title>Example Page</title></head>
///   <body>
///     <h1>Welcome to Example Page</h1>
///     <p>This is a paragraph with <strong>bold</strong> text.</p>
///     <ul>
///      <li>Item 1</li>
///      <li>Item 2</li>
///     </ul>
///   </body>
/// </html>
/// "#;
/// let clean_text = clean_content(&channel, content.to_string()).unwrap();
/// assert_eq!(clean_text, "# Welcome to Example Page\n\nThis is a paragraph with **bold** text.\n\n* Item 1\n* Item 2\n");
/// ```
pub fn clean_content(channel: &String, content: String) -> Result<String, PipelineError> {
    log::trace!("Cleaning content from channel: {}", channel);
    // Check that content is not empty
    if content.is_empty() {
        Err(PipelineError::EmptyString)
    } else {
        // Use html2text to filter paragraphs and lists
        // Parse the HTML
        let document = Document::from(content.as_str());

        // Extract the desired elements
        let mut extracted_html = String::new();

        // Extract the content depending on the feed
        match get_channel_type(channel) {
            ChannelType::ElPais => {
                // Extract the content from the article
                if let Some(article) = document.find(Name("article")).next() {
                    // print all attributes of the article
                    for attr in article.attrs() {
                        log::trace!(">>> Article attr: {:?}", attr);
                    }
                    for div in article.find(Name("div")) {
                        // print all attributes of the article
                        for attr in div.attrs() {
                            log::trace!(">>> Div attr: {:?}", attr);
                            if attr.0 == "data-dtm-region" && attr.1 == "articulo_cuerpo" {
                                for paragraph in div.find(Name("p")) {
                                    extracted_html.push_str(&paragraph.html());
                                }
                            }
                        }
                    }
                }
            }
            ChannelType::VeinteMinutos => {
                // Extract the content from the article of class "article-body"
                if let Some(article) = document.find(Name("article")).next() {
                    if let node::Data::Text(text) = article.data() {
                        log::trace!("Article text: {}", text);
                    }
                    for paragraph in article.find(Name("p")) {
                        extracted_html.push_str(&paragraph.html());
                    }
                }
            }
            ChannelType::ElDiario => {
                log::trace!("Channel is ElDiario");
                // Extract the content from the article
                if let Some(article) = document.find(Name("main")).next() {
                    for paragraph in article.find(Name("p")) {
                        for attr in paragraph.attrs() {
                            log::trace!(">>> Paragraph attr: {:?}", attr);
                            if attr.0 == "class" && attr.1 == "article-text" {
                                extracted_html.push_str(&paragraph.html());
                            }
                        }
                    }
                } else {
                    // Extract the content from the body
                    log::trace!("No main found!!!");
                    if let Some(body) = document.find(Name("body")).next() {
                        extracted_html.push_str(&body.html());
                    }
                }
            }
            ChannelType::ElMundo => {
                // Extract the content from the article
                if let Some(article) = document.find(Name("article")).next() {
                    for paragraph in article.find(Name("p")) {
                        extracted_html.push_str(&paragraph.html());
                    }
                } else {
                    // Extract the content from the body
                    log::trace!("No article found!!!");
                    if let Some(body) = document.find(Name("body")).next() {
                        extracted_html.push_str(&body.html());
                    }
                }
            }
            _ => {
                // Extract the content from the body
                if let Some(body) = document.find(Name("body")).next() {
                    extracted_html.push_str(&body.html());
                }
            }
        }

        // Use html2text to clean the html
        let clean_result = config::plain().string_from_read(Cursor::new(extracted_html), 1000);

        if let Ok(clean_text) = clean_result {
            Ok(clean_text)
        } else {
            Err(PipelineError::ParsingError(clean_result.err().unwrap().to_string()))
        }
    }
}

/// Function that using rqwest gets the content of a NewsItem passed as a
/// reference
pub async fn fill_news_item_content(news_item: &mut NewsItem) {
    let response = reqwest::get(&news_item.link).await;
    if let Ok(response) = response {
        let content = response.text().await;
        if let Ok(content) = content {
            let clean_content = clean_content(&news_item.channel, content);
            match clean_content {
                Ok(clean_content) => {
                    // If clean_content is not empty, assign it to the news_item
                    if !clean_content.is_empty() {
                        news_item.clean_content = Some(clean_content);
                    } else {
                        log::error!(
                            "Could not clean the content from {}. ERROR: {}",
                            news_item.link,
                            "Empty content"
                        );
                        news_item.clean_content = None;
                        news_item.error = Some(PipelineError::NoContent);
                    }
                }
                Err(err) => {
                    log::error!("Could not clean the content from {}. ERROR: {:?}", news_item.link, err);
                    news_item.clean_content = None;
                    news_item.error = Some(err);
                }
            }
        } else {
            let error = content.err().unwrap().to_string();
            log::error!("Could parse the content from {}. ERROR: {}", news_item.link, error);
            news_item.clean_content = None;
            news_item.error = Some(PipelineError::ParsingError(error));
        }
    } else {
        let error = response.err().unwrap().to_string();
        log::error!("Could not get the content from {}. ERROR: {}", news_item.link, error);
        news_item.clean_content = None;
        news_item.error = Some(PipelineError::NetworkError(error));
    }
}

// /// Function that returns the top k news items from the database
// pub async fn top_k_news_items(top_k: u8, news_items: &Vec<NewsItem>) ->
// Vec<NewsItem> {     // The top k news items are the ones with the highest
// relevance     let mut news_items = news_items.clone();
//     news_items.sort_by(|a, b| {
//         let relevance_a = calculate_relevance(a);
//         let relevance_b = calculate_relevance(b);
//         relevance_b.cmp(&relevance_a)
//     });

//     news_items.into_iter().take(top_k as usize).collect()
// }

/// Function that returns the top k news items
pub async fn top_k_news_items(top_k: u8, news_items: &Vec<NewsItem>) -> Vec<NewsItem> {
    // The top k news items are the ones with the highest relevance
    let mut handles = Vec::new();

    for item in news_items.iter() {
        let item = item.clone();
        // Spawn a blocking task for each relevance calculation
        let handle = tokio::spawn(async move {
            let relevance = calculate_relevance(&item).await;
            (item, relevance)
        });
        handles.push(handle);
    }

    let mut items_with_relevance = Vec::new();
    for handle in handles {
        if let Ok(result) = handle.await {
            items_with_relevance.push(result);
        }
    }

    // Sort items by relevance

    items_with_relevance.sort_by(|a, b| b.1.cmp(&a.1));

    // Take the top k items
    items_with_relevance
        .into_iter()
        .take(top_k as usize)
        .map(|(item, _)| item)
        .collect()
}

/// Function that logs a vector of NewsItems to a file appending the contents
pub fn log_news_items_to_file(news_items: &Vec<NewsItem>, file: &str) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .unwrap();

    // Order the news items by relevance
    let mut news_items = news_items.clone();
    news_items.sort_by(|a, b| b.relevance.cmp(&a.relevance));

    // Write table of contents
    writeln!(file, "# Table of Contents").unwrap();
    for (i,item) in news_items.iter().enumerate() {
        writeln!(file, "{}. [{}]({})", i + 1, item.title, generate_anchor(&item.title)).unwrap();
    }
    writeln!(file).unwrap();

    for item in news_items {
        writeln!(file,"---").unwrap();
        writeln!(file, "# {}", item.title).unwrap();
        writeln!(file, "## Data").unwrap();
        writeln!(file, "- **Channel:** {}", item.channel).unwrap();
        writeln!(file, "- **Relevance:** {}", item.relevance.unwrap_or_default()).unwrap();
        writeln!(file, "- **Link:** {}", item.link).unwrap();
        writeln!(file, "- **Publish Date:** {:?}", item.pub_date).unwrap();
        writeln!(file, "- **Categories:** {:?}", item.categories).unwrap();
        writeln!(file, "- **Keywords:** {:?}", item.keywords).unwrap();
        writeln!(file, "- **Error:** {:?}", item.error).unwrap();
        writeln!(file, "## Description\n{}", &item.description.chars().take(50).collect::<String>()).unwrap();
        match &item.clean_content {
            Some(clean_content) => {
                writeln!(file, "## Clean Content \n{}", clean_content).unwrap();
            }
            None => {
                writeln!(file, "## Clean Content \nN/A").unwrap();
            }
        }
        writeln!(file).unwrap();
        
    }
}

/// Function that generates an anchor from a title
fn generate_anchor(title: &str) -> String {
    // Convert to lowercase
    let mut anchor = title.to_lowercase();
    // Transliterate to ASCII
    // anchor = deunicode(&anchor);
    // Replace non-alphanumeric characters with hyphens
    let re = Regex::new(r"[^a-z0-9]+").unwrap();
    anchor = re.replace_all(&anchor, "-").to_string();
    // Trim leading and trailing hyphens
    anchor = anchor.trim_matches('-').to_string();
    format!("#{}", anchor)
}

/// Function that generates a dossier with a vector of news items
pub fn generate_dossier(news_items: &Vec<NewsItem>, file: &str) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .unwrap();

    // Header of the dossier
    writeln!(file, "# Dossier").unwrap();

    // Write table of contents
    writeln!(file, "## Table of Contents").unwrap();
    for (i,item) in news_items.iter().enumerate() {
        writeln!(file, "{}. [{}]({})", i + 1, item.title, generate_anchor(&item.title)).unwrap();
    }
    writeln!(file).unwrap();

    // Write metadata of the dossier
    writeln!(file, "## Metadata").unwrap();
    writeln!(file, "- **Number of items:** {}", news_items.len()).unwrap();
    writeln!(file, "- **Date:** {:?}", chrono::Local::now()).unwrap();
    writeln!(file).unwrap();

    // Write the news items
    writeln!(file, "## News Items").unwrap();

    for item in news_items {
        writeln!(file,"---").unwrap();
        writeln!(file, "### {}", item.title).unwrap();
        writeln!(file).unwrap();

        writeln!(file, "#### Data").unwrap();
        writeln!(file, "- **Channel:** {}", item.channel).unwrap();
        writeln!(file, "- **Relevance:** {}", item.relevance.unwrap_or_default()).unwrap();
        writeln!(file, "- **Link:** {}", item.link).unwrap();
        writeln!(file, "- **Publish Date:** {:?}", item.pub_date).unwrap();
        writeln!(file, "- **Categories:** {:?}", item.categories).unwrap();
        writeln!(file, "- **Keywords:** {:?}", item.keywords).unwrap();
        writeln!(file, "- **Error:** {:?}", item.error).unwrap();
        writeln!(file).unwrap();
        
        // writeln!(file, "#### Description\n{}", &item.description).unwrap();
        // writeln!(file).unwrap();

        match &item.clean_content {
            Some(clean_content) => {
                writeln!(file, "#### Clean Content \n{}", clean_content).unwrap();
            }
            None => {
                writeln!(file, "#### Clean Content \nN/A").unwrap();
            }
        }
        writeln!(file).unwrap();
        
    }
}

/// Function that logs vector of NewsItems into a sqlite database
pub async fn log_news_items_to_db(news_items: &Vec<NewsItem>, db_file_name: &str) -> usize {
    // Open a connection to the database
    let connection = sqlite::open(db_file_name).unwrap();

    // Create the table
    NewsItem::create_table(&connection).unwrap();

    // Insert the news items into the database
    let unique_inserted_items = insert_news_items(&news_items, &connection);

    unique_inserted_items
}

/// Function that inserts a vector of NewsItems into a database
pub fn insert_news_items(news_items: &Vec<NewsItem>, connection: &sqlite::Connection) -> usize {
    let mut count = 0;
    for news_item in news_items {
        match news_item.insert(connection) {
            Err(err) => {
                log::error!(
                    "Could not insert the NewsItem -> channel: {} link: {}. ERROR: {}",
                    news_item.channel,
                    news_item.link,
                    err.message.unwrap_or("no message".to_string())
                );
            }
            _ => {
                count += 1;
            }
        }
    }
    count
}

/// Function that given a vector of NewsItems fills the clean_content field of all of them
pub async fn fill_news_items_with_clean_contents(
    news_items: &mut Vec<NewsItem>,
) -> Option<Vec<NewsItem>> {
    let mut clean_news_items = Vec::new();

    // Calculate the number of tasks to spawn
    let tasks = news_items.len();

    log::trace!("Spawning {} tasks", tasks);

    let mut handles = vec![];
    for _ in 0..tasks {
        let mut news_item = news_items.pop().unwrap();
        let handle = tokio::spawn(async move {
            fill_news_item_content(&mut news_item).await;
            news_item
        });
        handles.push(handle);
    }

    // Wait for all the tasks to finish
    for handle in handles {
        clean_news_items.push(handle.await.unwrap());
    }
    

    if clean_news_items.is_empty() {
        None
    } else {
        Some(clean_news_items)
    }
}

/// Function that given a vector of NewsItems and the max number of threads to
/// spawn, calculates the relevance of each NewsItem and returns the updated
/// vector of NewsItems
pub async fn update_news_items_with_relevance(
    news_items: &mut Vec<NewsItem>,
) -> Option<Vec<NewsItem>> {
    log::info!("Updating relevance of {} news items", news_items.len());
    let mut updated_news_items = Vec::new();

    // Calculate the number of tasks to spawn
    let tasks = news_items.len();

    // Spawn as many thread as the minimum of max number of threads and the number
    // of urls and get the handles
    let mut handles = vec![];
    for _ in 0..tasks {
        let mut news_item = news_items.pop().unwrap();
        let handle = tokio::spawn(async move {
            let relevance = calculate_relevance(&news_item).await;
            log::debug!("Relevance of {} is {}", news_item.title, relevance.to_string());
            news_item.relevance = Some(relevance.net_relevance());
            news_item
        });
        handles.push(handle);
    }

    // Wait for all the tasks to finish
    for handle in handles {
        updated_news_items.push(handle.await.unwrap());
    }

    if updated_news_items.is_empty() {
        None
    } else {
        Some(updated_news_items)
    }
}

// Function that updates the news items with the calculated relevance and
// returns the top k items
pub async fn update_news_items_with_relevance_top_k(
    items: &mut Vec<NewsItem>,
    k: usize,
) -> Vec<NewsItem> {
    // Start time
    let start = std::time::Instant::now();

    // Update all the items with the calculated relevance
    let mut updated_items = update_news_items_with_relevance(items)
        .await
        .expect("Should not happen");

    log::info!(
        "Items updated {} in {} secs",
        updated_items.len(),
        start.elapsed().as_secs()
    );

    // Order updated items by relevance and take the top 100
    updated_items.sort_by(|a, b| b.relevance.cmp(&a.relevance));

    // Take the top k items
    let top_k_items = updated_items.into_iter().take(k).collect::<Vec<NewsItem>>();

    log::info!("Top K items: {}", top_k_items.len());

    top_k_items
}

#[cfg(test)]
mod tests {
    use super::*;
    // use regex::Regex;
    use rss::extension::{ExtensionBuilder, ExtensionMap};
    use rss::CategoryBuilder;
    use std::{collections::BTreeMap, io::Write};

    ///! Function that reads a feed from a file
    fn read_feed_from_file(file: &str) -> Result<Channel, Box<dyn Error>> {
        let file = std::fs::File::open(file)?;
        let reader = std::io::BufReader::new(file);
        let channel = Channel::read_from(reader)?;
        Ok(channel)
    }

    // This test checks that the function read_urls reads the urls from a file
    // It creates a file with 3 urls and checks that the function reads them
    // correctly
    #[test]
    fn test_read_urls() {
        // Write three urls to a file
        let urls = vec![
            "https://feeds.elpais.com/mrss-s/pages/ep/site/elpais.com/portada",
            "https://www.20minutos.es/rss/",
            "https://www.eldiario.es/rss/",
        ];
        let file = ".feeds.txt";
        let mut file = std::fs::File::create(file).unwrap();
        for url in urls.iter() {
            writeln!(file, "{}", url).unwrap();
        }

        let urls = read_urls(".feeds.txt").unwrap();
        assert_eq!(urls.len(), 3);
    }

    // This test checks that from_item creates a NewsItem from an RSS Item
    // It creates an RSS Item and checks that the function creates a NewsItem with
    // the correct values
    #[test]
    fn test_from_item() {
        // Create an RSS media extension for keywords using the ExtensionBuilder
        let keywords = ExtensionBuilder::default()
            .name("media:keywords")
            .value(Some("Keyword 1,Keyword 2".to_string()))
            .build();

        // Create an ExtensionMap with the media:keywords extension using a ValueBuilder
        let mut keywords_map = BTreeMap::new();
        keywords_map.insert("keywords".to_string(), vec![keywords]);
        let mut extensions = ExtensionMap::default();
        extensions.insert("media".to_string(), keywords_map);

        // Create a couple of test categories
        let category1 = CategoryBuilder::default().name("Category 1").build();
        let category2 = CategoryBuilder::default().name("Category 2").build();

        // Create a test Item with title, link, description and categories
        let item = rss::ItemBuilder::default()
            .title(Some("Title 1".to_string()))
            .link(Some("https://www.acme.es/section/uri-to-item.html".to_string()))
            .description(Some("Description".to_string()))
            .categories(vec![category1.clone(), category2.clone()])
            .extensions(extensions)
            .build();

        // Create a channel adding the item
        let channel = rss::ChannelBuilder::default()
            .title("My RSS Feed")
            .link("https://example.com")
            .description("This is an example RSS feed")
            .items(vec![item.clone()])
            .build();

        log::trace!("Channel: {:?}", channel.to_string());

        let news_item = NewsItem::from_item(&channel.title(), &item).unwrap();
        assert_eq!(news_item.title, "Title 1");
        assert_eq!(news_item.link, "https://www.acme.es/section/uri-to-item.html");
        assert_eq!(news_item.description, "Description");
        assert_eq!(
            news_item.categories,
            Some("Category 1,Category 2".to_lowercase().to_string())
        );
        assert_eq!(
            news_item.keywords,
            Some("Keyword 1,Keyword 2".to_lowercase().to_string())
        );
    }

    // This test checks that an Item without the media:keywords extension is
    // correctly converted to a NewsItem
    #[test]
    fn test_from_item_no_keywords() {
        // Create a couple of test categories
        let category1 = CategoryBuilder::default().name("Category 1").build();
        let category2 = CategoryBuilder::default().name("Category 2").build();

        // Create a test Item with title, link, description and categories
        let item = rss::ItemBuilder::default()
            .title(Some("Title 1".to_string()))
            .link(Some("https://www.acme.es/section/uri-to-item.html".to_string()))
            .description(Some("Description".to_string()))
            .categories(vec![category1.clone(), category2.clone()])
            .build();

        let news_item = NewsItem::from_item("Other", &item).unwrap();
        assert_eq!(news_item.title, "Title 1");
        assert_eq!(news_item.link, "https://www.acme.es/section/uri-to-item.html");
        assert_eq!(news_item.description, "Description");
        assert_eq!(
            news_item.categories,
            Some("Category 1,Category 2".to_lowercase().to_string())
        );
        assert_eq!(news_item.keywords, None);
    }

    // This test checks that an Item without the categories is correctly converted
    // to a NewsItem
    #[test]
    fn test_from_item_no_categories() {
        // Create a test Item with title, link, description and categories
        let item = rss::ItemBuilder::default()
            .title(Some("Title 1".to_string()))
            .link(Some("https://www.acme.es/section/uri-to-item.html".to_string()))
            .description(Some("Description".to_string()))
            .build();

        let news_item = NewsItem::from_item("Other", &item).unwrap();
        assert_eq!(news_item.title, "Title 1");
        assert_eq!(news_item.link, "https://www.acme.es/section/uri-to-item.html");
        assert_eq!(news_item.description, "Description");
        assert_eq!(news_item.categories, None);
        assert_eq!(news_item.keywords, None);
    }

    // This test checks that an Item without title, link or description fails to
    // convert to a NewsItem
    #[test]
    fn test_from_item_no_title_link_description() {
        // Create a test Item with title, link, description and categories
        let item = rss::ItemBuilder::default().build();

        let news_item = NewsItem::from_item("Other", &item);
        assert_eq!(news_item.is_err(), true);
    }

    // Test that the from_item function works for a test file
    #[test]
    fn test_from_item_from_test_file() {
        let channel = read_feed_from_file("tests/feed.xml").unwrap();

        // Get items from the channel
        let items = channel.items();

        // There have to be 1 item
        assert_eq!(items.len(), 1);

        log::trace!("Item: {:?}", &items[0]);

        // Convert the item to news item
        let news_item = NewsItem::from_item("Other", &items[0]).unwrap();

        log::trace!("NewsItem: {:?}", news_item);

        assert_eq!(news_item.title, "Title 1");
        assert_eq!(news_item.link, "https://www.acme.es/section/uri-to-item.html");
        assert_eq!(news_item.description, "Description");
        assert_eq!(
            news_item.categories,
            Some("Category 1,Category 2".to_lowercase().to_string())
        );
        assert_eq!(
            news_item.keywords,
            Some("Keyword 1, Keyword 2".to_lowercase().to_string())
        );
    }

    // Test cleam_html function with an empty string
    #[test]
    fn test_clean_content_with_empty_string() {
        let channel = "Other".to_string();
        let html = "";
        let clean_text = clean_content(&channel, html.to_string());
        assert!(clean_text.is_err());
    }

    // Test cleam_html function with bad formatted html
    #[test]
    fn test_clean_content_with_bad_formatted() {
        let channel = "Other".to_string();
        let html = r#"
        <ht>
        <bo dy>
        This is a Heading
        <p>This is a paragraph</p>
        </bo dy
        "#;

        let clean_text = clean_content(&channel, html.to_string()).unwrap();
        assert_eq!(clean_text, "This is a Heading\n\nThis is a paragraph\n");
    }

    // Test cleam_html function with emojis and special characters
    #[test]
    fn test_clean_content_with_emojis_special_chars() {
        let channel = "Other".to_string();
        let html = r#"
        <html>
        <head><title>Example Page</title></head>
        <body>
        <h1>Welcome to Example Page</h1>
        <p>This is a paragraph with <strong>bold</strong> text.</p>
        <p>This is a paragraph with <em>italic</em> text.</p>
        <p>This is a paragraph with <em>italic</em> text and an emoji 😊.</p>
        <p>Párrafo con caracteres especiales como: ñéåîü€@.</p>
        <ul>
        <li>Item one</li>
        <li>Item two</li>
        <li>Item three</li>
        </ul>
        </body>
        </html>
        "#;

        let clean_text = clean_content(&channel, html.to_string()).unwrap();
        assert_eq!(
            clean_text,
            "# Welcome to Example Page\n\nThis is a paragraph with **bold** text.\n\nThis is a paragraph with *italic* text.\n\nThis is a paragraph with *italic* text and an emoji 😊.\n\nPárrafo con caracteres especiales como: ñéåîü€@.\n\n* Item one\n* Item two\n* Item three\n"
        );
    }
}
