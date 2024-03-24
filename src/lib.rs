///! Library that provides functions to read and parse RSS feeds
use std::{error::Error, io::BufRead};

use rss::Channel;

/// Struct that represents a News Item
#[derive(Debug)]
pub struct NewsItem {
    pub title: String,
    pub link: String,
    pub description: String,
    pub categories: String,
    pub keywords: Option<String>,
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
            .map(|category| category.name().to_string())
            .collect::<Vec<String>>()
            .join(",");
        let extensions = item.extensions().clone();
        println!(">>> Extensions: {:?}", extensions);
        let keywords = extensions.get("media").and_then(|ext| {
            ext.get("keywords").and_then(|extensions| {
                extensions
                    .iter()
                    .map(|ext| {
                        if ext.name == "media:keywords" && ext.value.is_some() {
                            Some(ext.value().unwrap().to_string())
                        } else {
                            None
                        }
                    })
                    .collect::<Option<Vec<String>>>()
            })
        });
        let keywords = keywords.and_then(|keywords| Some(keywords.join(", ")));
        Ok(NewsItem {
            title,
            link,
            description,
            categories,
            keywords: keywords,
        })
    }
}

///! Function that reads a feed from a URL
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
    let urls: Vec<String> = reader.lines().map(|line| line.unwrap()).collect();
    Ok(urls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, io::Write};
    use rss::CategoryBuilder;
    use rss::extension::{ExtensionBuilder, ExtensionMap};

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

        println!("Channel: {:?}", channel.to_string());

        let news_item = NewsItem::from_item(&item).unwrap();
        assert_eq!(news_item.title, "Title 1");
        assert_eq!(news_item.link, "https://www.acme.es/section/uri-to-item.html");
        assert_eq!(news_item.description, "Description");
        assert_eq!(news_item.categories, "Category 1,Category 2");
        assert_eq!(news_item.keywords, Some("Keyword 1,Keyword 2".to_string()));
    }

    // Test that the from_item function works for a test file
    #[test]
    fn test_from_item_from_test_file() {
        let channel = read_feed_from_file("tests/feed.xml").unwrap();

        // Get items from the channel
        let items = channel.items();

        // There have to be 1 item
        assert_eq!(items.len(), 1);

        println!("Item: {:?}", &items[0]);

        // Convert the item to news item
        let news_item = NewsItem::from_item(&items[0]).unwrap();

        println!("NewsItem: {:?}", news_item);

        assert_eq!(news_item.title, "Title 1");
        assert_eq!(news_item.link, "https://www.acme.es/section/uri-to-item.html");
        assert_eq!(news_item.description, "Description");
        assert_eq!(news_item.categories, "Category 1,Category 2");
        assert_eq!(news_item.keywords, Some("Keyword 1, Keyword 2".to_string()));
    }
}
