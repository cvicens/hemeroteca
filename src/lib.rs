//! Library that provides functions to read and parse RSS feeds

pub mod common;
pub mod embeddings;
pub mod openai;
pub mod relevance;
pub mod storage;

// Re-export commonly used items in a prelude module
pub mod prelude {
    pub use crate::calculate_relevance_of_newsitem;
    pub use crate::clean_content;
    pub use crate::common::ChannelType;
    pub use crate::common::FeedbackRecord;
    pub use crate::common::NewsItem;
    pub use crate::common::Operator;
    pub use crate::common::PipelineError;
    pub use crate::embeddings::{DEFAULT_MODEL_ID, DEFAULT_REVISION};
    pub use crate::fetch_news_items_opted_in;
    pub use crate::fill_news_item_content;
    pub use crate::fill_news_items_with_clean_contents;
    pub use crate::generate_dossier_report;
    pub use crate::generate_feedback_records;
    pub use crate::generate_relevance_report;
    pub use crate::get_channel_type;
    pub use crate::insert_news_items;
    pub use crate::log_news_items_to_db;
    pub use crate::log_news_items_to_file;
    pub use crate::log_report_to_file;
    pub use crate::read_feed;
    pub use crate::read_urls;
    pub use crate::storage::{
        read_feedback_records_from_parquet, write_feedback_records_parquet, write_feedback_records_to_csv,
    };
    pub use crate::top_k_news_items;
    pub use crate::update_news_items_with_relevance;
    pub use crate::update_news_items_with_relevance_top_k;
    pub use crate::update_relevance_of_news_items;
}

use crate::relevance::calculate_relevance;
use candle_core::Tensor;
use common::{ChannelType, FeedbackRecord, NewsItem, Operator, PipelineError};
use embeddings::{
    build_model_and_tokenizer, cosine_similarity, generate_embeddings, generate_embeddings_for_sentences,
};

use std::io::{BufRead, Cursor, Write};

use select::predicate::Name;
use select::{document::Document, node};

use html2text::config;
use rand::seq::SliceRandom;
use regex::Regex;
use rss::{Channel, Item};

/// Function that reads a feed from a URL
pub async fn read_feed(feed_url: &str) -> anyhow::Result<Channel> {
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
pub fn read_urls(file: &str) -> anyhow::Result<Vec<String>> {
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
    feed_urls: &[String],
    opt_in: &[String],
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
            // Map the result to an option and log the error if any
            channel
                .map_err(|e| {
                    log::error!("Could not read the feed from {}. ERROR: {}", url, e);
                })
                .ok()
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
            // Maps option channel to an option tuple of the title and the items
            channel
                .as_ref()
                .map(|channel| (channel.title(), channel.items().to_vec()))
        })
        .collect();

    // If there are no items return None
    if items.is_empty() {
        None
    } else {
        // Else, get all the items from the channels
        let mut all_items: Vec<NewsItem> = items
            .iter()
            .flat_map(|(channel, items)| {
                items
                    .iter()
                    .map(|item| NewsItem::from_item(channel, item))
                    .filter_map(|result| {
                        if let Ok(result) = result {
                            Some(result)
                        } else {
                            log::error!("Could not get the item from the result: {:?}", result);
                            None
                        }
                    })
                    .collect::<Vec<NewsItem>>()
            })
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
pub fn get_channel_type(channel: &str) -> ChannelType {
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

/// Function that returns the top k news items based on their relevance
pub async fn top_k_news_items(top_k: u8, news_items: &[NewsItem]) -> Vec<NewsItem> {
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
pub fn log_news_items_to_file(news_items: &[NewsItem], file: &str) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .unwrap();

    // Order the news items by relevance
    let mut news_items = news_items.to_owned();
    news_items.sort_by(|a, b| a.cmp_relevance(b));

    // Write table of contents
    writeln!(file, "# Table of Contents").unwrap();
    for (i, item) in news_items.iter().enumerate() {
        writeln!(file, "{}. [{}]({})", i + 1, item.title, generate_anchor(&item.title)).unwrap();
    }
    writeln!(file).unwrap();

    for item in news_items {
        writeln!(file, "---").unwrap();
        writeln!(file, "# {}", item.title).unwrap();
        writeln!(file, "## Data").unwrap();
        writeln!(file, "- **Channel:** {}", item.channel).unwrap();
        writeln!(file, "- **Relevance:** {}", item.relevance.unwrap_or_default()).unwrap();
        writeln!(file, "- **Link:** {}", item.link).unwrap();
        writeln!(file, "- **Publish Date:** {:?}", item.pub_date).unwrap();
        writeln!(file, "- **Categories:** {:?}", item.categories).unwrap();
        writeln!(file, "- **Keywords:** {:?}", item.keywords).unwrap();
        writeln!(file, "- **Error:** {:?}", item.error).unwrap();
        writeln!(
            file,
            "## Description\n{}",
            &item.description.chars().take(50).collect::<String>()
        )
        .unwrap();
        match &item.clean_content {
            Some(clean_content) => {
                writeln!(file, "## Clean Content \n{}", clean_content).unwrap();
            }
            None => {
                writeln!(file, "## Clean Content N/A").unwrap();
            }
        }
        writeln!(file).unwrap();
    }
}

/// Function that generates a relevance report from a vector of NewsItems as a String
pub fn generate_relevance_report(news_items: &[NewsItem]) -> String {
    let mut report = String::new();

    // Order the news items by relevance
    let mut news_items = news_items.to_owned();
    news_items.sort_by(|a, b| b.cmp_relevance(a));

    // Write table of contents
    report.push_str("# Relevance Report\n");
    report.push('\n');

    // Prepare a bucket for the relevance of the news items per channel
    let mut relevance_per_channel = std::collections::HashMap::new();

    // Sum the relevance of the news items per channel
    for item in news_items.iter() {
        let relevance = item.relevance.unwrap_or_default();
        let channel = item.channel.clone();

        let channel_relevance = relevance_per_channel.entry(channel).or_insert((0.0, 0));
        channel_relevance.0 += relevance;
        channel_relevance.1 += 1;
    }

    // Order the channels by average relevance
    let mut relevance_per_channel: Vec<_> = relevance_per_channel.into_iter().collect();
    relevance_per_channel.sort_by(|a, b| {
        let relevance_a = a.1 .0 / a.1 .1 as f64;
        let relevance_b = b.1 .0 / b.1 .1 as f64;
        relevance_b.partial_cmp(&relevance_a).unwrap()
    });

    // Write the relevance per channel
    report.push_str("## Relevance per Channel\n\n");
    for (channel, relevance) in relevance_per_channel.iter() {
        report.push_str(&format!(
            "- **{}:** Items: {} Total: {} Average: {}\n",
            channel,
            relevance.1,
            relevance.0,
            relevance.0 / relevance.1 as f64
        ));
    }
    report.push('\n');

    // Write the news items with their relevance
    report.push_str("## Relevance list\n");
    for (i, item) in news_items.iter().enumerate() {
        let relevance = item.relevance.unwrap_or_default();
        report.push_str(&format!("{}. ({}) [{}] {}\n", i, relevance, item.channel, item.title));
    }

    report
}

/// Function that writes a relevance report to a file and returns a Result
pub async fn log_relevance_report_to_file(report: &str, file: &str) -> anyhow::Result<()> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(file)?;

    writeln!(file, "{}", report)?;

    Ok(())
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

/// Function that writes a report (&str) to a file and returns a Result
pub async fn log_report_to_file(report: &str, file: &str) -> anyhow::Result<()> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(file)?;

    writeln!(file, "{}", report)?;

    Ok(())
}

/// Function that generates a dossier report with a vector of news items
pub fn generate_dossier_report(news_items: &Vec<NewsItem>) -> String {
    let mut report = String::new();

    // Header of the dossier
    report.push_str("# Dossier\n\n");

    // Write table of contents
    report.push_str("## Table of Contents\n");
    for (i, item) in news_items.iter().enumerate() {
        report.push_str(&format!(
            "{}. [{}]({})\n",
            i + 1,
            item.title,
            generate_anchor(&item.title)
        ));
    }
    report.push('\n');

    // Write metadata of the dossier
    report.push_str("## Metadata\n");
    report.push_str(&format!("- **Number of items:** {}\n", news_items.len()));
    report.push_str(&format!("- **Date:** {:?}\n", chrono::Local::now()));
    report.push('\n');

    // Write the news items
    report.push_str("## News Items\n");

    for item in news_items {
        report.push_str("---\n");
        report.push_str(&format!("### {}\n", item.title));
        report.push('\n');

        report.push_str("#### Data\n");
        report.push_str(&format!("- **Channel:** {}\n", item.channel));
        report.push_str(&format!("- **Relevance:** {}\n", item.relevance.unwrap_or_default()));
        report.push_str(&format!("- **Link:** {}\n", item.link));
        report.push_str(&format!("- **Publish Date:** {:?}\n", item.pub_date));
        report.push_str(&format!("- **Categories:** {:?}\n", item.categories));
        report.push_str(&format!("- **Keywords:** {:?}\n", item.keywords));
        report.push_str(&format!("- **Error:** {:?}\n", item.error));
        report.push('\n');

        // report.push_str("#### Description\n{}", &item.description);
        // report.push_str(file);

        match &item.clean_content {
            Some(clean_content) => {
                report.push_str(&format!("#### Clean Content \n{}", clean_content));
            }
            None => {
                report.push_str("#### Clean Content N/A");
            }
        }
        report.push('\n');
    }

    report
}

/// Function that logs vector of NewsItems into a sqlite database
pub async fn log_news_items_to_db(news_items: &Vec<NewsItem>, db_file_name: &str) -> usize {
    // Open a connection to the database
    let connection = sqlite::open(db_file_name).unwrap();

    // Create the table
    NewsItem::create_table(&connection).unwrap();

    // Insert the news items into the database
    insert_news_items(news_items, &connection)
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
pub async fn fill_news_items_with_clean_contents(news_items: &mut Vec<NewsItem>) -> Option<Vec<NewsItem>> {
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
pub async fn update_news_items_with_relevance(news_items: &mut Vec<NewsItem>) -> Option<Vec<NewsItem>> {
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
pub async fn update_news_items_with_relevance_top_k(items: &mut Vec<NewsItem>, k: usize) -> Vec<NewsItem> {
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
    // updated_items.sort_by(|a, b| b.relevance.cmp(&a.relevance));
    updated_items.sort_by(|a, b| {
        match (b.relevance, a.relevance) {
            (Some(b_relevance), Some(a_relevance)) => b_relevance.partial_cmp(&a_relevance).unwrap(),
            (None, Some(_)) => std::cmp::Ordering::Greater, // Treat `None` as smallest
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });

    // Take the top k items
    let top_k_items = updated_items.into_iter().take(k).collect::<Vec<NewsItem>>();

    log::info!("Top K items: {}", top_k_items.len());

    top_k_items
}

/// Function given a slice of NewsItems return a Vec of FeedbackRecords
///
/// Example:
///
/// ```rust
/// use hemeroteca::generate_feedback_records;
/// use hemeroteca::prelude::NewsItem;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
///
/// let news_item_1 = NewsItem {
///     error: None,
///     creators: "Creator".to_string(),
///     categories: Some("Politics".to_string()),
///     keywords: Some("Elections".to_string()),
///     title: "President Elections".to_string(),
///     description: "Description".to_string(),
///     clean_content: None,
///     channel: "Channel".to_string(),
///     link: "http://example.com".to_string(),
///     pub_date: None,
///     relevance: None,
/// };
/// let news_item_2 = NewsItem {
///     error: None,
///     creators: "Creator".to_string(),
///     categories: Some("IA".to_string()),
///     keywords: Some("Inteligencia Artificial".to_string()),
///     title: "New LLM model released".to_string(),
///     description: "".to_string(),
///     clean_content: None,
///     channel: "Channel".to_string(),
///     link: "http://example.com".to_string(),
///     pub_date: None,
///     relevance: None,
/// };
/// let news_items = vec![news_item_1, news_item_2];
/// let model_id = hemeroteca::prelude::DEFAULT_MODEL_ID.to_string();
/// let revision = hemeroteca::prelude::DEFAULT_REVISION.to_string();
/// let gpu = false;
/// let use_pth = false;
/// let normalize_embedding = false;
/// let approximate_gelu = false;
/// let feedback_records = generate_feedback_records(&news_items, &model_id, &revision, gpu, use_pth, normalize_embedding, approximate_gelu).await.unwrap();
///
/// assert_eq!(feedback_records.len(), 2);
/// # }
/// ```
pub async fn generate_feedback_records(
    news_items: &[NewsItem],
    model_id: &str,
    model_revision: &str,
    gpu: bool,
    use_pth: bool,
    normalize_embedding: bool,
    approximate_gelu: bool,
) -> anyhow::Result<Vec<FeedbackRecord>> {
    // Spawn tokio task for each NewsItem to generate embeddings for title and keywords+categories
    let mut handles = Vec::new();
    for item in news_items.iter() {
        // Skip items with errors
        if item.error.is_some() {
            log::warn!("Skipping item with error: {:?}", item.error);
            continue;
        }
        // Clone the item to move it into the async block
        let item = item.clone();
        let model_id = model_id.to_string();
        let revision = model_revision.to_string();
        // Spawn a blocking task for each item to generate embeddings a couple of embeddings (title and keywords+categories)
        let handle: tokio::task::JoinHandle<Result<FeedbackRecord, anyhow::Error>> = tokio::spawn(async move {
            let (model, tokenizer) = build_model_and_tokenizer(&model_id, &revision, gpu, use_pth, approximate_gelu)?;
            // Concatenate keywords and categories separated by spaces
            let bow = item.get_bow();
            let sentences = vec![item.title.as_str(), bow.as_str()];
            let embeddings = generate_embeddings(tokenizer, model, &sentences, normalize_embedding).await?;

            // Check that the embeddings have the correct dimensions
            assert!(embeddings.dims()[0] == 2);

            // Extract the title and keywords+categories embeddings
            let title_embedding: Vec<f32> = embeddings.narrow(0, 0, 1)?.to_vec2()?[0].clone();
            log::trace!("title_embedding: {:?}", title_embedding);

            let bow_embedding: Vec<f32> = embeddings.narrow(0, 1, 1)?.to_vec2()?[0].clone();
            log::trace!("bow_embedding: {:?}", bow_embedding);

            Ok(FeedbackRecord { news_item: item, title_embedding, bow_embedding })
        });
        handles.push(handle);
    }

    let mut feedback_records = Vec::new();
    for handle in handles {
        if let Ok(result) = handle.await {
            // Push the feedback record if the handle was Ok
            feedback_records.push(result?);
        }
    }

    Ok(feedback_records)
}

/// Functon that updates the relevance of a slice of NewsItems given a slice of FeedbackRecords
pub async fn update_relevance_of_news_items(
    news_items: &mut [NewsItem],
    feedback_records: &[FeedbackRecord],
    similarity_threshold: f32,
    model_id: &str,
    revision: &str,
    gpu: bool,
    use_pth: bool,
    approximate_gelu: bool,
    normalize_embedding: bool,
) -> anyhow::Result<()> {
    log::info!("Updating relevance of {} news items", news_items.len());
    log::info!("Using {} of feedback records", feedback_records.len());

    // Spawn a tokio task for each NewsItem to calculate the relevance
    let mut handles = Vec::new();
    for news_item in news_items.iter_mut() {
        // Skip items with errors
        if news_item.error.is_some() {
            log::warn!("Skipping item with error: {:?}", news_item.error);
            continue;
        }
        // Clone to move it into the async block
        let news_item = news_item.clone();
        let model_id = model_id.to_string();
        let revision = revision.to_string();
        let feedback_records = feedback_records.to_vec();
        // Spawn a blocking task for each item to calculate the relevance
        let handle: tokio::task::JoinHandle<Result<f64, anyhow::Error>> = tokio::spawn(async move {
            let relevance = calculate_relevance_of_newsitem(
                &news_item,
                &feedback_records,
                similarity_threshold,
                &model_id,
                &revision,
                gpu,
                use_pth,
                approximate_gelu,
                normalize_embedding,
            )
            .await?;
            Ok(relevance)
        });
        handles.push(handle);
    }

    // Wait for all the tasks to finish
    for (i, handle) in handles.into_iter().enumerate() {
        if let Ok(result) = handle.await {
            news_items[i].relevance = Some(result?);
            log::debug!(
                "Relevance of {} is {}",
                news_items[i].title,
                news_items[i].relevance.unwrap()
            );
        }
    }

    Ok(())
}

/// Function that calculates the relevance of a NewsItem by similarity given a slice of FeedbackRecords
pub async fn calculate_relevance_of_newsitem(
    news_item: &NewsItem,
    feedback_records: &[FeedbackRecord],
    similarity_threshold: f32,
    model_id: &str,
    revision: &str,
    gpu: bool,
    use_pth: bool,
    approximate_gelu: bool,
    normalize_embedding: bool,
) -> anyhow::Result<f64> {
    // Start time
    let start = std::time::Instant::now();

    // Calculate embedding for the title of the news item
    let (model, tokenizer) = build_model_and_tokenizer(model_id, revision, gpu, use_pth, approximate_gelu)?;
    let embeddings = generate_embeddings_for_sentences(
        tokenizer,
        model,
        &[&news_item.title, &news_item.get_bow()],
        normalize_embedding,
    )
    .await?;

    // Calculate relevance with regards to the join of keywords and categories
    let title_relevance =
        calculate_relevance_by_cosine_similarity(&embeddings[0], feedback_records, similarity_threshold, |record| {
            (record.title_embedding, record.news_item.relevance.unwrap_or_default())
        })
        .await?;
    let bow_relevance =
        calculate_relevance_by_cosine_similarity(&embeddings[1], feedback_records, similarity_threshold, |record| {
            (record.title_embedding, record.news_item.relevance.unwrap_or_default())
        })
        .await?;

    let elapsed_time = start.elapsed().as_secs_f64();
    log::debug!("Relevance calculated in {} secs", elapsed_time);

    // Reduce the relevance 10% for each day that has passed since the publication date to a maximum of 30%
    let mut relevance = title_relevance.max(bow_relevance);
    if let Some(pub_date) = news_item.pub_date.clone() {
        let pub_date = chrono::DateTime::parse_from_rfc2822(&pub_date)
            .unwrap()
            .with_timezone(&chrono::Local);
        let days = (chrono::Local::now() - pub_date).num_days();
        let decay = 0.1 * days as f64;
        relevance *= (1.0 - decay).max(0.7);
    }

    // Return the relevance
    Ok(relevance)
}

/// Function that calculates the relevance of a NewsItem by similarity to a slice of FeedbackRecords
async fn calculate_relevance_by_cosine_similarity(
    embedding: &Tensor,
    feedback_records: &[FeedbackRecord],
    threshold: f32,
    extractor: fn(FeedbackRecord) -> (Vec<f32>, f64),
) -> anyhow::Result<f64> {
    // Iterate over the feedback records, spawn a tokio task for each record to calculate the cosine similarity
    let mut tasks = Vec::new();
    for feedback_record in feedback_records {
        let record_embedding_with_relevance = extractor(feedback_record.clone());
        let task: tokio::task::JoinHandle<Result<(f32, f64), _>> = tokio::spawn({
            let embedding = embedding.clone();
            let record_embedding_with_relevance = record_embedding_with_relevance.clone(); // Move the data needed
                                                                                           // Check that the embeddings have the correct dimensions
            let embedding_length = embedding.dims()[1];
            let record_embedding_length = record_embedding_with_relevance.0.len();
            assert!(embedding_length == record_embedding_length);
            async move {
                let device = embedding.device();
                let record_embedding = Tensor::new(&record_embedding_with_relevance.0[..], device)?
                    .reshape(&[1, record_embedding_length])?;
                let relevance = record_embedding_with_relevance.1;
                let similarity = cosine_similarity::<f32>(&embedding, &record_embedding).await?;
                Ok::<(f32, f64), anyhow::Error>((similarity, relevance))
            }
        });
        tasks.push(task);
    }

    // Wait for all the tasks to finish
    let mut results = Vec::new();
    for task in tasks {
        results.push(task.await?);
    }

    // Filter out tuples which similarity value is below threshold
    let mut similarities: Vec<(f32, f64)> = results
        .into_iter() // Turn the original vector into an iterator
        .filter_map(|item: Result<(f32, f64), _>| {
            if let Ok((similarity, relevance)) = item {
                if similarity > threshold {
                    return Some((similarity, relevance)); // Keep this item
                }
            }
            None // Discard the item
        })
        .collect();

    // Order the similarities by similarity
    similarities.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    // If there are no similarities return 0
    if similarities.is_empty() {
        Ok(0.0)
    } else {
        // Calculate the average of the relevances
        let relevances: Vec<f64> = similarities.iter().map(|(_, relevance)| *relevance).collect();
        let avg_relevance = relevances.iter().sum::<f64>() / relevances.len() as f64;
        if avg_relevance.is_nan() || avg_relevance.is_infinite() || avg_relevance < 0.0 {
            Ok(0.0)
        } else {
            Ok(avg_relevance)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embeddings::{DEFAULT_MODEL_ID, DEFAULT_REVISION};
    // use regex::Regex;
    use rss::extension::{ExtensionBuilder, ExtensionMap};
    use rss::CategoryBuilder;
    use std::{collections::BTreeMap, io::Write};

    ///! Function that reads a feed from a file
    fn read_feed_from_file(file: &str) -> anyhow::Result<Channel> {
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

    // Test calculate_relevance_by_similarity_to_feedback_records with a couple of feedback records
    #[tokio::test]
    async fn test_calculate_relevance_by_similarity_to_feedback_records() -> anyhow::Result<()> {
        let similarity_threshold = 0.95;
        // Create a couple of feedback records
        let feedback_record_1 = FeedbackRecord {
            news_item: NewsItem {
                error: None,
                creators: "Creator".to_string(),
                categories: Some("Politics".to_string()),
                keywords: Some("Elections".to_string()),
                title: "President Elections".to_string(),
                description: "Description".to_string(),
                clean_content: None,
                channel: "Channel".to_string(),
                link: "http://example.com".to_string(),
                pub_date: None,
                relevance: Some(0.5),
            },
            title_embedding: vec![0.1; 384],
            bow_embedding: vec![0.5; 384],
        };
        let feedback_record_2 = FeedbackRecord {
            news_item: NewsItem {
                error: None,
                creators: "Creator".to_string(),
                categories: Some("IA".to_string()),
                keywords: Some("Inteligencia Artificial".to_string()),
                title: "New LLM model released".to_string(),
                description: "".to_string(),
                clean_content: None,
                channel: "Channel".to_string(),
                link: "http://example.com".to_string(),
                pub_date: None,
                relevance: Some(0.7),
            },
            title_embedding: vec![0.1; 384],
            bow_embedding: vec![0.5; 384],
        };
        let feedback_records = vec![feedback_record_1, feedback_record_2];

        // Create a test NewsItem
        let news_item = NewsItem {
            error: None,
            creators: "Creator".to_string(),
            categories: Some("Politics".to_string()),
            keywords: Some("Elections".to_string()),
            title: "President Elections".to_string(),
            description: "Description".to_string(),
            clean_content: None,
            channel: "Channel".to_string(),
            link: "http://example.com".to_string(),
            pub_date: None,
            relevance: None,
        };

        // Calculate the relevance of the NewsItem
        let relevance = calculate_relevance_of_newsitem(
            &news_item,
            &feedback_records,
            similarity_threshold,
            DEFAULT_MODEL_ID,
            DEFAULT_REVISION,
            false,
            false,
            false,
            false,
        )
        .await?;

        assert_eq!(relevance, 0.0);
        Ok(())
    }
}
