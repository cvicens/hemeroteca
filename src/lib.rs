///! Library that provides functions to read and parse RSS feeds
use std::{error::Error, io::BufRead};

use rss::Channel;

///! Function that reads a feed from a URL
pub async fn read_feed(feed_url: &str) -> Result<Channel, Box<dyn Error>> {
    let content = reqwest::get(feed_url).await?.bytes().await?;
    let channel = Channel::read_from(&content[..])?;
    Ok(channel)
}

///! Function that reads feed urls from a file
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
    use std::io::Write;
    
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
}
