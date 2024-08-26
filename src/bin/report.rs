use std::path::Path;

use hemeroteca::prelude::*;

use clap::{Parser, Subcommand, ValueEnum};

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

    /// Threads used to parse the feeds
    #[arg(short, long)]
    threads: Option<usize>,

    /// List of categories or keywords to opt in
    #[arg(short, long)]
    opt_in: Vec<String>,

    /// Operator to use for filtering only `and` and `or` are supported
    #[arg(long, default_value = "or")]
    operator: OptInOperator,

    // Subcommands
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// It generates a dossier
    Dossier {
        /// Report name
        #[arg(short, long, default_value = "report")]
        report_name: String,

        /// Log to file
        #[arg(short, long)]
        log: bool,

        /// Log to database
        #[arg(short, long)]
        db: bool,
    },

    // Relevance
    Relevance {
        /// Report name
        #[arg(short, long, default_value = "report")]
        report_name: String,
    },
}

/// Main function
fn main() {
    // Initialize the logger and set info as the default level and turn off html5ever logs
    env_logger::Builder::from_env(Env::default()
        .default_filter_or("info"))
        .filter_module("html5ever", log::LevelFilter::Off)
        .init();
    
    // env_logger::init();
    // env_logger::Builder::from_env(Env::default())
    //     .filter_module("html5ever", log::LevelFilter::Off)
    //     .init();


    // Read the 'feeds_file' argument using clap
    let args: Args = Args::parse();

    // Get the feed urls file name
    let feeds_file = args.feeds_file;

    // If the number of threads is not provided, use the number of cores
    let max_threads = args.threads.unwrap_or(num_cpus::get() as usize);

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
    let feed_urls = feed_urls.unwrap();
    log::info!("Feed urls to read: {:?}", feed_urls);

    // Create a tokio runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(max_threads as usize)
        .enable_all()
        .build()
        .unwrap();

    // If the command is dossier
    match args.command {
        Some(Commands::Dossier {report_name, log, db}) => {
            log::info!("Generating dossier with the report name: {}", report_name);
            rt.block_on( async {
                generate_dossier_command(&feed_urls, &report_name, opt_in, operator.as_wrapper(), log, db).await;
            });
        }
        Some(Commands::Relevance {report_name}) => {
            log::info!("Generating relevance with the report name: {}", report_name);
            rt.block_on( async {
                generate_relevance_command(&feed_urls, &report_name).await;
            });
        }
        None => {
            log::error!("No command provided. Exiting...");
        }
    }
    
}

/// Function that implements the relevance command
/// Arguments:
/// - feed_urls: Vec<String> - The feed urls to read
/// - report_name: String - The name of the report
async fn generate_relevance_command(feed_urls: &Vec<String>, report_name: &String) {
    // Vector to store the items read from the feeds
    let items = fetch_news_items_opted_in(feed_urls, &vec![], Operator::OR).await;

    // Get the current date in the format YYYY-MM-DD-HH-MM-SS
    let current_date = chrono::Local::now().format("%Y-%m-%d-%H-%M-%S").to_string();

    // if we could read the items from the feeds
    if let Some(mut items) = items {
        log::info!("Items read from the feeds: {:?}", items.len());

        // Update all the items with the calculated relevance
        let updated_items = update_news_items_with_relevance(&mut items).await.expect("Should not happen");

        // Create the report folder name
        let report_folder = format!("{}_{}", report_name, current_date);

        // Define the folder path
        let folder_path = Path::new(&report_folder);

        // Create the report folder
        std::fs::create_dir_all(folder_path).expect("Could not create the report folder!");

        // Create the report file name
        let report_file = folder_path.join(format!("relevance-{}_{}.md", report_name, current_date));

        // Logging to file
        log::info!("Logging to the report log file: {}", report_file.to_str().unwrap());

        // Generate the relevance report
        let relevance_report = generate_relevance_report(&updated_items);

        // Log relevance report to output file
        if let Err(err) = log_relevance_report_to_file(&relevance_report, report_file.to_str().unwrap()).await {
            log::error!("Failed to log relevance report to file: {}", err);
        }
    } else {
        log::error!("No news items found! Exiting...");
    }
}

/// Function that implements the dossier command
/// Arguments:
/// - feed_urls: Vec<String> - The feed urls to read
/// - report_name: String - The name of the report
/// - opt_in: Vec<String> - The categories to filter in
/// - operator: Operator - The operator to use for filtering
/// - log: bool - Whether to log to file
/// - db: bool - Whether to log to database
async fn generate_dossier_command(feed_urls: &Vec<String>, report_name: &String, opt_in: Vec<String>, operator: Operator, log: bool, db: bool) {
    // Vector to store the items read from the feeds
    let items = fetch_news_items_opted_in(feed_urls, &opt_in, operator).await;

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

            // Create the report folder name
            let report_folder = format!("{}_{}", report_name, current_date);

            // Define the folder path
            let folder_path = Path::new(&report_folder);

            // Create the report folder
            std::fs::create_dir_all(folder_path).expect("Could not create the report folder!");

            // If log is true, log to file
            if log {
                // Create the log file name
                let report_log_file = folder_path.join(format!("{}_{}.md", report_name, current_date));

                // Logging to file
                log::info!("Logging to the report log file: {}", report_log_file.to_str().unwrap());

                // Log clean news items to output file
                log_news_items_to_file(&clean_news_items, report_log_file.to_str().unwrap());
            }

            // If db is true, log to database
            if db {
                // Create the report db file name
                let report_db_file = folder_path.join(format!("{}_{}.db", report_name, current_date));
                
                // Logging to database
                log::info!("Logging to the report log database: {}", report_db_file.to_str().unwrap());

                // Insert the news items into the database
                let unique_inserted_items = log_news_items_to_db(&clean_news_items, report_db_file.to_str().unwrap()).await;
                log::info!("Unique inserted items: {:?}", unique_inserted_items);
            }

            // Now that the contents are present and clean pdate again all the items with the calculated relevance 
            // and return the top k items
            let top_k_items = update_news_items_with_relevance_top_k(&mut clean_news_items, 20).await;

            
            // Create the dossier file name
            let dossier_file = folder_path.join(format!("dossier-{}_{}.md", report_name, current_date));

            // Generating dossier
            log::info!("Generating dossier: {}", dossier_file.to_str().unwrap());

            // Generate the dossier with the top k items
            generate_dossier(&top_k_items, dossier_file.to_str().unwrap());
            
        } else {
            log::error!("No news items survived the cleaning phase! Exiting...");
        }
    }
}

