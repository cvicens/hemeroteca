
use hemeroteca::prelude::*;

use clap::{Parser, ValueEnum};

use env_logger::Env;

// OptInOperator enum
#[derive(Debug, Clone, ValueEnum)]
enum OptInOperator {
    AND,
    OR,
}

impl OptInOperator {
    fn as_wrapper(&self) -> Operator {
        match self {
            OptInOperator::AND => Operator::AND,
            OptInOperator::OR => Operator::OR,
        }
    }
}

// CLAP Arguments Parsing
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// File with the feeds to read
    #[arg(short, long, default_value = "feeds.txt")]
    feeds_file: String,

    /// Report name
    #[arg(long, default_value = "hemeroteca")]
    report_name: String,

    /// File to output the news items
    #[arg(long, default_value = "output.txt")]
    output_file: String,

    /// Threads used to parse the feeds
    #[arg(short, long, default_value = "4")]
    threads: u8,

    /// List of categories or keywords to opt in
    #[arg(short, long)]
    opt_in: Vec<String>,

    /// Operator to use for filtering only `and` and `or` are supported
    #[arg(long, default_value = "or")]
    operator: OptInOperator,
}

#[tokio::main]
async fn main() {
    // Initialize the logger and turn off html5ever logs
    // env_logger::init();
    env_logger::Builder::from_env(Env::default())
    .filter_module("html5ever", log::LevelFilter::Off)
    .init();

    // Read the 'feeds_file' argument using clap
    let args: Args = Args::parse();

    // Get the feed urls file name
    let feeds_file = args.feeds_file;

    // Get the report name
    let report_name = args.report_name;

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

    // Get the operator to use for filtering
    let operator = args.operator;

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
    let items = fetch_news_items_opted_in(&mut feed_urls, max_threads, &opt_in, operator.as_wrapper()).await;

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

            // Get the current date in the format YYYY-MM-DD-HH-MM-SS
            let current_date = chrono::Local::now().format("%Y-%m-%d-%H-%M-%S").to_string();

            // Create the report db file name
            let report_db_file = format!("{}_{}.db", report_name, current_date);

            // Store in the database
            log::info!("Storing the news items in the database: {}", report_db_file);

            // Open a connection to the database
            let connection = sqlite::open(report_db_file).unwrap();

            // Create the table
            NewsItem::create_table(&connection).unwrap();

            // Insert the news items into the database
            let unique_inserted_items = insert_news_items(&clean_news_items, &connection);
            log::info!("Unique inserted items: {:?}", unique_inserted_items);

            // Return the top K news items
            let top_k = 10;
            let top_news_items = top_k_news_items(top_k, &clean_news_items).await;

            // Print the top news items
            log::info!("Top {} news items:", top_k);
            for (i, news_item) in top_news_items.iter().enumerate() {
                log::info!("{}. {}", i + 1, news_item.title);
            }

            // // Call summarize function
            // let summary = summarize();
            // print!("Summary: {}", summary);
        }
        else {
            log::error!("No news items survived the cleaning phase! Exiting...");
        }
    }
}
