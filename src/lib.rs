//! Library that provides functions to read and parse RSS feeds
use std::{
    error::Error,
    io::{BufRead, Cursor},
};

use html2text::config;
use rand::seq::SliceRandom;
use regex::Regex;
use rss::{Channel, Item};

/// Struct that represents a News Item
#[derive(Debug, Clone)]
pub struct NewsItem {
    pub title: String,
    pub link: String,
    pub description: String,
    pub categories: Option<String>,
    pub keywords: Option<String>,
    pub clean_content: Result<String, PipelineError>,
}

// Define a custom error type for the pipeline
#[derive(Debug, Clone)]
pub enum PipelineError {
    EmptyString,
    ParsingError(String),
    NoContent,
    NetworkError(String),
}

impl NewsItem {
    /// Function that creates a NewsItem from an RSS Item and returns a Result or Error
    ///
    /// Example:
    /// ```
    /// use rss::Item;
    /// use hemeroteca::NewsItem;
    ///
    /// let item = Item::default();
    /// let news_item = NewsItem::from_item(&item);
    /// assert_eq!(news_item.is_err(), true);
    /// ```
    pub fn from_item(item: &rss::Item) -> Result<NewsItem, Box<dyn Error>> {
        let title = item.title().ok_or("No title")?.to_string();
        let link = item.link().ok_or("No link")?.to_string();
        let description = item.description().ok_or("No description")?.to_string();
        let categories = item
            .categories()
            .iter()
            .map(|category| category.name().to_string().to_lowercase())
            .collect::<Vec<String>>();
        let categories = if categories.is_empty() {
            None
        } else {
            Some(categories.join(","))
        };
        let extensions = item.extensions().clone();
        let keywords = extensions.get("media").and_then(|ext| {
            ext.get("keywords")
                .and_then(|extensions| {
                    extensions
                        .iter()
                        .map(|ext| {
                            if ext.name == "media:keywords" && ext.value.is_some() {
                                Some(ext.value().unwrap().to_string().to_lowercase())
                            } else {
                                None
                            }
                        })
                        .collect::<Option<Vec<String>>>()
                })
                .map(|keywords| keywords.join(","))
        });
        Ok(NewsItem {
            title,
            link,
            description,
            categories,
            keywords,
            clean_content: Err(PipelineError::EmptyString),
        })
    }
}

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

/// Function that given a vector or feed urls and using read_feed get all the Items per Channel
/// and returns them in an optional vector
pub async fn get_all_items(
    feed_urls: &mut Vec<String>,
    max_threads: u8,
    opt_in: Vec<String>,
) -> Option<Vec<NewsItem>> {
    let mut items = Vec::new();
    // While there are urls to read
    while !feed_urls.is_empty() {
        // Calculate the number of threads to spawn
        let threads = std::cmp::min(max_threads as usize, feed_urls.len());

        // Spawn as many thread as the minimum of max number of threads and the number of urls and get the handles
        log::trace!("Spawning {} threads", threads);
        let mut handles = vec![];
        for _ in 0..threads {
            let url = feed_urls.pop().unwrap();
            let handle = tokio::spawn(async move {
                let channel = read_feed(&url).await;
                if channel.is_err() {
                    log::error!(
                        "Could not read the feed from {}. ERROR: {}",
                        url,
                        channel.err().unwrap()
                    );
                    vec![]
                } else {
                    let channel = channel.unwrap();
                    // Return those items that do not have any of the categories to filter out
                    let items: Vec<Item> = channel.items().to_vec();
                    log::trace!("> Read {} items from {}", items.len(), url);
                    items
                }
            });
            handles.push(handle);
        }

        // Wait for all the threads to finish
        for handle in handles {
            items.push(handle.await.unwrap());
        }
    }

    if items.is_empty() {
        None
    } else {
        // Flatten the items vectors
        let items: Vec<Item> = items.into_iter().flatten().collect();

        // Convert the items to NewsItems
        let mut items: Vec<_> = items
            .iter()
            .map(|item| NewsItem::from_item(item).unwrap())
            .collect();

        // Filter out the items that have any of the categories or keywords equal to the categories to filter out
        items.retain(|item| {
            let categories = item.categories.clone().unwrap_or("".to_string());
            let keywords = item.keywords.clone().unwrap_or("".to_string());
            log::trace!(
                "Checking {:?} in {:?} and {:?}",
                opt_in,
                categories,
                keywords
            );
            opt_in
                .iter()
                .any(|item| categories.contains(item) || keywords.contains(item))
        });

        // Shuffle the items
        items.shuffle(&mut rand::thread_rng());

        Some(items)
    }
}

// /// Function that using rqwest gets all the contents of all the urls of a vec of NewsItems passed as a reference
// pub async fn get_all_contents(news_items: &Vec<NewsItem>) {
//     let mut contents = Vec::new();
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
//             news_item.clean_content = Err(PipelineError::NetworkError(error));
//         }
//     }
// }

/// Function that using rqwest gets the content of a NewsItem passed as a reference
pub async fn fill_news_item_content(news_item: &mut NewsItem) {
    let response = reqwest::get(&news_item.link).await;
    if let Ok(response) = response {
        let content = response.text().await;
        if let Ok(content) = content {
            news_item.clean_content = html_to_text(content);
        } else {
            let error = content.err().unwrap().to_string();
            log::error!(
                "Could parse the content from {}. ERROR: {}",
                news_item.link,
                error
            );
            news_item.clean_content = Err(PipelineError::ParsingError(error));
        }    
    } else {
        let error = response.err().unwrap().to_string();
        log::error!(
            "Could not get the content from {}. ERROR: {}",
            news_item.link,
            error
        );
        news_item.clean_content = Err(PipelineError::NetworkError(error));
    }
}

/// Function that cleans the text content of an html string
///
/// Example:
/// ```
/// use hemeroteca::html_to_text;
///
/// let html = r#"
///    <html>
///       <head><title>Example Page</title></head>
///      <body>
///         <h1>Welcome to Example Page</h1>
///        <p>This is a paragraph with <strong>bold</strong> text.</p>
///       <ul>
///         <li>Item 1</li>
///        <li>Item 2</li>
///      </ul>
///    </body>
/// </html>
/// "#;
/// let clean_text = html_to_text(html.to_string()).unwrap();
/// assert_eq!(clean_text, "# Welcome to Example Page\n\nThis is a paragraph with **bold** text.\n\n* Item 1\n* Item 2\n");
/// ```
pub fn html_to_text(html: String) -> Result<String, PipelineError> {
    // Check that html is not empty
    if html.is_empty() {
        Err(PipelineError::EmptyString)
    } else {
        // Use html2text to clean the html
        let clean_result = config::plain().string_from_read(Cursor::new(html), 1000);

        if let Ok(clean_text) = clean_result {
            Ok(clean_text)
        } else {
            Err(PipelineError::ParsingError(
                clean_result.err().unwrap().to_string(),
            ))
        }
    }
}

/// Function that given a vector of NewsItems and the max number of threads to spawn, fills the clean_content field of the NewsItems
pub async fn fill_news_items_with_clean_contents(
    news_items: &mut Vec<NewsItem>,
    max_threads: u8
) -> Option<Vec<NewsItem>> {
    let mut clean_news_items = Vec::new();
    // While there are contents to clean
    while !news_items.is_empty() {
        // Calculate the number of threads to spawn
        let threads = std::cmp::min(max_threads as usize, news_items.len());

        // Spawn as many thread as the minimum of max number of threads and the number of urls and get the handles
        log::trace!("Spawning {} threads", threads);
        let mut handles = vec![];
        for _ in 0..threads {
            let mut news_item = news_items.pop().unwrap();
            let handle = tokio::spawn(async move {
                fill_news_item_content(&mut news_item).await;
                news_item
            });
            handles.push(handle);
        }

        // Wait for all the threads to finish
        for handle in handles {
            clean_news_items.push(handle.await.unwrap());
        }
    }

    if clean_news_items.is_empty() {
        None
    } else {
        Some(clean_news_items)
    }
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
    // It creates a file with 3 urls and checks that the function reads them correctly
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
    // It creates an RSS Item and checks that the function creates a NewsItem with the correct values
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
            .link(Some(
                "https://www.acme.es/section/uri-to-item.html".to_string(),
            ))
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

        let news_item = NewsItem::from_item(&item).unwrap();
        assert_eq!(news_item.title, "Title 1");
        assert_eq!(
            news_item.link,
            "https://www.acme.es/section/uri-to-item.html"
        );
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

    // This test checks that an Item without the media:keywords extension is correctly converted to a NewsItem
    #[test]
    fn test_from_item_no_keywords() {
        // Create a couple of test categories
        let category1 = CategoryBuilder::default().name("Category 1").build();
        let category2 = CategoryBuilder::default().name("Category 2").build();

        // Create a test Item with title, link, description and categories
        let item = rss::ItemBuilder::default()
            .title(Some("Title 1".to_string()))
            .link(Some(
                "https://www.acme.es/section/uri-to-item.html".to_string(),
            ))
            .description(Some("Description".to_string()))
            .categories(vec![category1.clone(), category2.clone()])
            .build();

        let news_item = NewsItem::from_item(&item).unwrap();
        assert_eq!(news_item.title, "Title 1");
        assert_eq!(
            news_item.link,
            "https://www.acme.es/section/uri-to-item.html"
        );
        assert_eq!(news_item.description, "Description");
        assert_eq!(
            news_item.categories,
            Some("Category 1,Category 2".to_lowercase().to_string())
        );
        assert_eq!(news_item.keywords, None);
    }

    // This test checks that an Item without the categories is correctly converted to a NewsItem
    #[test]
    fn test_from_item_no_categories() {
        // Create a test Item with title, link, description and categories
        let item = rss::ItemBuilder::default()
            .title(Some("Title 1".to_string()))
            .link(Some(
                "https://www.acme.es/section/uri-to-item.html".to_string(),
            ))
            .description(Some("Description".to_string()))
            .build();

        let news_item = NewsItem::from_item(&item).unwrap();
        assert_eq!(news_item.title, "Title 1");
        assert_eq!(
            news_item.link,
            "https://www.acme.es/section/uri-to-item.html"
        );
        assert_eq!(news_item.description, "Description");
        assert_eq!(news_item.categories, None);
        assert_eq!(news_item.keywords, None);
    }

    // This test checks that an Item without title, link or description fails to convert to a NewsItem
    #[test]
    fn test_from_item_no_title_link_description() {
        // Create a test Item with title, link, description and categories
        let item = rss::ItemBuilder::default().build();

        let news_item = NewsItem::from_item(&item);
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
        let news_item = NewsItem::from_item(&items[0]).unwrap();

        log::trace!("NewsItem: {:?}", news_item);

        assert_eq!(news_item.title, "Title 1");
        assert_eq!(
            news_item.link,
            "https://www.acme.es/section/uri-to-item.html"
        );
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
    fn test_html_to_text_with_empty_string() {
        let html = "";
        let clean_text = html_to_text(html.to_string());
        assert!(clean_text.is_err());
    }

    // Test cleam_html function with bad formatted html
    #[test]
    fn test_html_to_text_with_bad_formatted() {
        let html = r#"
        <ht>
        <bo dy>
        This is a Heading
        <p>This is a paragraph</p>
        </bo dy
        "#;

        let clean_text = html_to_text(html.to_string()).unwrap();
        assert_eq!(clean_text, "This is a Heading\n\nThis is a paragraph\n");
    }

    // Test cleam_html function with emojis and special characters
    #[test]
    fn test_html_to_text_with_emojis_special_chars() {
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

        let clean_text = html_to_text(html.to_string()).unwrap();
        assert_eq!(
            clean_text,
            "# Welcome to Example Page\n\nThis is a paragraph with **bold** text.\n\nThis is a paragraph with *italic* text.\n\nThis is a paragraph with *italic* text and an emoji 😊.\n\nPárrafo con caracteres especiales como: ñéåîü€@.\n\n* Item one\n* Item two\n* Item three\n"
        );
    }
}
