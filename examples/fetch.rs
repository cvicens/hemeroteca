use hemeroteca::prelude::*;
use tokio::main;

#[main]
async fn main() {
    // Vector of feed urls
    let feed_urls = vec![
        "https://www.eldiario.es/rss".to_string(),
    ];

    // Vector of categories to filter out
    let opt_in = vec![];
    
    // Operator to use for filtering
    let operator = Operator::OR;

    let items = fetch_news_items_opted_in(&feed_urls, &opt_in, operator).await;
    match items {
        Some(items) => {
            for item in items {
                println!("Title: {}", item.title);
                println!("Link: {}", item.link);
                println!("Description: {}", item.description);
                println!("Pub Date: {:?}", item.pub_date);
                println!("Categories: {:?}", item.categories);
                println!("---");
            }
        }
        None => {
            println!("No items found");
        }
    }
}