use hemeroteca::{get_all_contents, get_all_items, read_urls, NewsItem};

use clap::Parser;

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
    let feed_urls = read_urls(&file);

    // If we could not read the urls from the file, print the error and return
    if let Err(err) = feed_urls {
        log::error!("Could not read the urls from {}. ERROR: {}", file, err);
        return;
    }

    // Unwrap urls
    let mut feed_urls = feed_urls.unwrap();

    // Vector to store the items read from the feeds
    let items = get_all_items(&mut feed_urls, max_threads, opt_in).await;

    if let Some(items) = items {
        // Get the first 5 news items
        let first_five_items = items.iter().take(5).cloned().collect();

        // // Print the first 5 news items
        // for item in first_five_items {
        //     println!("NewsItem >>: {:?}", item);
        //     println!();
        // }

        // Get the contents of the items collected
        let contents = get_all_contents(&first_five_items).await;

        // Print the contents
        for content in contents {
            println!("Content >>: {:?}", content);
            println!();
        }
    }
}
