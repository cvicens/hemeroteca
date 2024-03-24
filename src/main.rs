use hemeroteca::{read_feed, read_urls};

use clap::Parser;
use rss::Item;
use rand::seq::SliceRandom;

// CLAP Arguments Parsing
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// File with the feeds to read
    #[arg(short, long, default_value = "feeds.txt")]
    file: String,

    /// Threads used to parse the feeds
    #[arg(short, long, default_value = "4")]
    threads: u8,

    /// List of categories to filter out
    #[arg(short, long)]
    categories: Vec<String>,
}

#[tokio::main]
async fn main() {
    // Read the 'file' argument using clap
    let args: Args = Args::parse();

    // Get the feed urls file name
    let file = args.file;

    // Get the number of threads
    let max_threads = args.threads;

    // Get the categories to filter out
    let categories = args.categories;

    // Read the feed urls from the file
    let mut urls = read_urls(&file).unwrap();

    // Vector to store the items read from the feeds
    let mut items = vec![];

    // While there are urls to read
    while !urls.is_empty() {
        // Print the number of urls read
        println!("Read {} urls from {}", urls.len(), file);

        // Calculate the number of threads to spawn
        let threads = std::cmp::min(max_threads as usize, urls.len());

        // Spawn as many thread as the minimum of max number of threads and the number of urls and get the handles
        println!("Spawning {} threads", threads);
        let mut handles = vec![];
        for _ in 0..threads {
            let url = urls.pop().unwrap();
            let categories_clone = categories.clone(); // Clone categories
            let handle = tokio::spawn(async move {
                let channel = read_feed(&url).await.unwrap();
                
                // Return those items that do not have any of the categories to filter out
                let items: Vec<Item> = channel.items().iter().filter(|item| {
                    !categories_clone.iter().any(|category| item.categories().iter().any(|cat| cat.name().eq_ignore_ascii_case(category)))
                }).cloned().collect(); // Clone the items before collecting them
                println!("> Read {} items from {}", items.len(), url);
                items
            });
            handles.push(handle);
        }

        // Wait for all the threads to finish
        for handle in handles {
            items.push(handle.await.unwrap());
        }
    }

    // Flatten the items vectors
    let mut items: Vec<Item> = items.into_iter().flatten().collect();

    // Shuffle the items
    items.shuffle(&mut rand::thread_rng());

    // Print 1 items
    for item in items.iter().take(1) {
        println!("Item >>: {:?}", item);
        // println!("Title: {}", item.title);
        // println!("Link: {}", item.link);
        // println!("Description: {}", item.description);
        // println!("Categories: {:?}", item.categories);
        println!();
    }
    
    // Convert the items to NewsItems
    let items: Vec<_> = items.iter().map(|item| hemeroteca::NewsItem::from_item(item).unwrap()).collect();

    // Print 1 items
    for item in items.iter().take(1) {
        println!("NewsItem >>: {:?}", item);
        // println!("Title: {}", item.title);
        // println!("Link: {}", item.link);
        // println!("Description: {}", item.description);
        // println!("Categories: {:?}", item.categories);
        println!();
    }
}
