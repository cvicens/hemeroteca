
use hemeroteca::{insert_news_items, prelude::*};

use clap::Parser;

use env_logger::Env;

// CLAP Arguments Parsing
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// File with the feeds to read
    #[arg(short, long, default_value = "feeds.txt")]
    feeds_file: String,

    /// File to output the news items
    #[arg(long, default_value = "output.txt")]
    output_file: String,

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
    // env_logger::init();
    env_logger::Builder::from_env(Env::default())
    .filter_module("html5ever", log::LevelFilter::Off)
    .init();

    // Read the 'feeds_file' argument using clap
    let args: Args = Args::parse();

    // Get the feed urls file name
    let feeds_file = args.feeds_file;

    // Get the output file name
    let output_file = args.output_file;

    // Get the number of threads
    let max_threads = args.threads;

    // Get the categories to filter out in lowercase
    let opt_in = args
        .opt_in
        .into_iter()
        .map(|s| s.to_lowercase())
        .collect::<Vec<String>>();
    log::info!("Filtering in: {:?}", opt_in);

    // If no opt_in is provided, print a warning and exit
    if opt_in.is_empty() {
        log::error!("No categories provided to filter out. Exiting...");
        return;
    }

    // Read the feed urls from the file
    let feed_urls = read_urls(&feeds_file);
    log::info!("Reading feed urls from the file: {}", feeds_file);

    // If we could not read the urls from the file, print the error and return
    if let Err(err) = feed_urls {
        log::error!("Could not read the urls from {}. ERROR: {}", feeds_file, err);
        return;
    }

    // Unwrap urls
    let mut feed_urls = feed_urls.unwrap();
    log::info!("Feed urls to read: {:?}", feed_urls);

    // Vector to store the items read from the feeds
    let items = get_all_items(&mut feed_urls, max_threads, opt_in).await;

    // if we could read the items from the feeds
    if let Some(mut items) = items {
        log::info!("Items read from the feeds: {:?}", items.len());

        // Fill the news items with clean contents
        let clean_news_items = fill_news_items_with_clean_contents(&mut items, max_threads).await;

        // Write intermidiate results to the file
        if let Some(clean_news_items) = clean_news_items {
            log::info!("Clean news items: {:?}", clean_news_items.len());

            // Log clean news items to output file
            log_news_items_to_file(&clean_news_items, &output_file);

            // Store in the database

            // Open a connection to the database
            let connection = sqlite::open("hemeroteca.db").unwrap();

            // Create the table
            NewsItem::create_table(&connection).unwrap();

            // Insert the news items into the database
            let unique_inserted_items = insert_news_items(&clean_news_items, &connection);
            log::info!("Unique inserted items: {:?}", unique_inserted_items);

            // Call summarize function
            // let summary = summarize();
            // print!("Summary: {}", summary);
        }
        else {
            log::error!("No news items survived the cleaning phase! Exiting...");
        }
    }
}
