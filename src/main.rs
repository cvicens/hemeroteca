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
        log::info!("No categories provided to filter out. Getting all the news items from feeds.");
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
    let items = fetch_news_items_opted_in(&mut feed_urls, &opt_in, operator.as_wrapper()).await;

    // if we could read the items from the feeds
    if let Some(mut items) = items {
        log::info!("Items read from the feeds: {:?}", items.len());

        // Update all the items with the calculated relevance and return the top k items
        let mut top_k_items = update_news_items_with_relevance_top_k(&mut items, 100).await;

        // Fill the news items with clean contents
        let clean_news_items = fill_news_items_with_clean_contents(&mut top_k_items).await;

        // Write intermediate results to the file
        if let Some(mut clean_news_items) = clean_news_items {
            log::info!("Clean news items: {:?}", clean_news_items.len());

            // Get the current date in the format YYYY-MM-DD-HH-MM-SS
            let current_date = chrono::Local::now().format("%Y-%m-%d-%H-%M-%S").to_string();

            // Create the report db file name
            let report_db_file = format!("{}_{}.db", report_name, current_date);

            // Create the log file name
            let report_log_file = format!("{}_{}.md", report_name, current_date);

            // Logging to file
            log::info!("Logging to the report log file: {}", report_log_file);

            // Log clean news items to output file
            log_news_items_to_file(&clean_news_items, &report_log_file);

            // Logging to file
            log::info!("Logging to the report log database: {}", report_db_file);

            // Insert the news items into the database
            let unique_inserted_items = log_news_items_to_db(&clean_news_items, &report_db_file);
            log::info!("Unique inserted items: {:?}", unique_inserted_items);

            // Now that the contents are present and clean pdate again all the items with the calculated relevance 
            // and return the top k items
            let top_k_items = update_news_items_with_relevance_top_k(&mut clean_news_items, 20).await;

            
            // Create the dossier file name
            let dossier_file = format!("dossier-{}_{}.md", report_name, current_date);

            // Generate the dossier with the top k items
            generate_dossier(&top_k_items, &dossier_file);
            
        } else {
            log::error!("No news items survived the cleaning phase! Exiting...");
        }
    }
}
