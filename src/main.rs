use hemeroteca::{read_feed, read_urls};

use clap::Parser;
use rand::seq::SliceRandom;
use rss::Item;

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

    /// List of categories or keywords to filter out
    #[arg(short, long)]
    opt_in: Vec<String>,
}

#[tokio::main]
async fn main() {
    // Initialize the logger
    env_logger::init();

    // Read the 'file' argument using clap
    let args: Args = Args::parse();

    // Get the feed urls file name
    let file = args.file;

    // Get the number of threads
    let max_threads = args.threads;

    // Get the categories to filter out in lowercase
    let opt_in = args
        .opt_in
        .into_iter()
        .map(|s| s.to_lowercase())
        .collect::<Vec<String>>();
    log::debug!("Filtering out: {:?}", opt_in);

    // Read the feed urls from the file
    let urls = read_urls(&file);

    // If we could not read the urls from the file, print the error and return
    if let Err(err) = urls {
        log::error!("Could not read the urls from {}. ERROR: {}", file, err);
        return;
    }

    // Unwrap urls
    let mut urls = urls.unwrap();

    // Vector to store the items read from the feeds
    let mut items = vec![];

    // While there are urls to read
    while !urls.is_empty() {
        // Print the number of urls read
        log::debug!("Read {:?} urls from {}", urls, file);

        // Calculate the number of threads to spawn
        let threads = std::cmp::min(max_threads as usize, urls.len());

        // Spawn as many thread as the minimum of max number of threads and the number of urls and get the handles
        log::trace!("Spawning {} threads", threads);
        let mut handles = vec![];
        for _ in 0..threads {
            let url = urls.pop().unwrap();
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
                    let items: Vec<Item> = channel
                        .items()
                        .iter()
                        .cloned() // Clone the items before collecting them
                        .collect();
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

    // Flatten the items vectors
    let mut items: Vec<Item> = items.into_iter().flatten().collect();

    // Shuffle the items
    items.shuffle(&mut rand::thread_rng());

    // Convert the items to NewsItems
    let mut items: Vec<_> = items
        .iter()
        .map(|item| hemeroteca::NewsItem::from_item(item).unwrap())
        .collect();

    // Filter out the items that have any of the categories or keywords equal to the categories to filter out
    items.retain(|item| {
        let categories = item.categories.clone().unwrap_or("".to_string());
        let keywords = item.keywords.clone().unwrap_or("".to_string());
        log::trace!(
            "Checking {:?} in {:?} and {:?}",
            opt_in, categories, keywords
        );
        opt_in
            .iter()
            .any(|item| categories.contains(item) || keywords.contains(item))
    });

    // How many items are left
    log::debug!("Matched {} items", items.len());

    // Print 1 items
    for item in items.iter().take(1) {
        println!("NewsItem >>: {:?}", item);
        println!();
    }
}
