use hemeroteca::{fill_news_items_with_clean_contents, get_all_items, read_urls};

use clap::Parser;
use std::fs::OpenOptions;
use std::io::Write;

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
    log::debug!("Filtering in: {:?}", opt_in);

    // If no opt_in is provided, print a warning and exit
    if opt_in.is_empty() {
        log::error!("No categories provided to filter out. Exiting...");
        return;
    }

    // Read the feed urls from the file
    let feed_urls = read_urls(&feeds_file);

    // If we could not read the urls from the file, print the error and return
    if let Err(err) = feed_urls {
        log::error!("Could not read the urls from {}. ERROR: {}", feeds_file, err);
        return;
    }

    // Unwrap urls
    let mut feed_urls = feed_urls.unwrap();
    log::debug!("Feed URLs: {:?}", feed_urls);

    // Vector to store the items read from the feeds
    let items = get_all_items(&mut feed_urls, max_threads, opt_in).await;

    if let Some(mut items) = items {
        // Truncate the vec to the first 5
        items.truncate(100);

        // // Print the first 5 news items
        // for item in first_five_items {
        //     println!("NewsItem >>: {:?}", item);
        //     println!();
        // }

        // Get the contents of the items collected
        // let contents = get_all_contents(&items).await;

        // Clean the contents
        let clean_news_items = fill_news_items_with_clean_contents(&mut items, max_threads).await;

            // ...

        if let Some(clean_news_items) = clean_news_items {
            let mut file = OpenOptions::new()
                .create(true) // Create if it doesn't exist
                .append(true) // Append to the file
                .open(output_file).unwrap();
            // Write the contents to the file
            for item in clean_news_items {
                // If clean_content is not an error, write it to the file
                // It clean_content is not an error, write it to the file
                if let Ok(clean_content) = item.clean_content {
                    writeln!(file, "=======================================================================").unwrap();
                    writeln!(file, "channel: {}", item.channel).unwrap();
                    writeln!(file, "title: {}", item.title).unwrap();
                    writeln!(file, "link: {}", item.link).unwrap();
                    writeln!(file, "description: {}", item.description).unwrap();
                    writeln!(file, "clean_content: {}", clean_content).unwrap();
                } else {
                    // If it is an error, print the error
                    log::error!("Could not write the content to the file. ERROR: {:?}", item.clean_content);
                }
            }
            
        }
        else {
            log::error!("No news items survived the cleaning phase! Exiting...");
        }
    }
}
