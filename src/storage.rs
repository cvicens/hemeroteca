/// Module for storage related functions
use crate::common::{FeedbackRecord, NewsItem, PipelineError};

use arrow::buffer::Buffer;
use csv::Writer;
use sqlite::{Connection, State};

use std::fs::File;
use std::str::FromStr;
use std::sync::Arc;

use arrow::array::{ArrayData, ArrayRef, FixedSizeListArray, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;

/// NewsItem related functions
impl NewsItem {
    /// Function that returns a Bindable slice of tuples with the values of the
    /// NewsItem
    ///
    /// Example:
    ///
    /// ```
    /// use hemeroteca::prelude::*;
    ///
    /// let news_item = NewsItem {
    ///    channel: "Channel".to_string(),
    ///    title: "Title".to_string(),
    ///    link: "Link".to_string(),
    ///    description: "Description".to_string(),
    ///    pub_date: Some("Date".to_string()),
    ///    categories: Some("Categories".to_string()),
    ///    keywords: Some("Keywords".to_string()),
    ///    clean_content: Some("Clean Content".to_string()),
    ///    creators: "John Doe".to_string(),
    ///    error: None,
    ///    relevance: None,
    /// };
    ///
    /// let binds = news_item.binds();
    /// assert_eq!(binds.len(), 10);
    /// ```
    pub fn binds(&self) -> [(&str, &str); 10] {
        let pub_date = match &self.pub_date {
            Some(date) => date.as_str(),
            None => "",
        };
        let categories = match &self.categories {
            Some(c) => c.as_str(),
            None => "",
        };
        let keywords = match &self.keywords {
            Some(k) => k.as_str(),
            None => "",
        };
        let clean_content = match &self.clean_content {
            Some(c) => c.as_str(),
            None => "",
        };

        let error = match &self.error {
            Some(e) => e.as_str(),
            None => "None",
        };

        [
            (":channel", self.channel.as_str()),
            (":title", self.title.as_str()),
            (":link", self.link.as_str()),
            (":description", self.description.as_str()),
            (":creators", self.creators.as_str()),
            (":pub_date", pub_date),
            (":categories", categories),
            (":keywords", keywords),
            (":clean_content", clean_content),
            (":error", error),
        ]
    }

    /// Function that create a table in the database to store the news items
    ///
    /// Example:
    /// ```
    /// use hemeroteca::prelude::*;
    ///
    /// let conn = sqlite::open(":memory:").unwrap();
    /// let result = NewsItem::create_table(&conn);
    /// assert_eq!(result.is_ok(), true);
    /// ```
    pub fn create_table(conn: &Connection) -> sqlite::Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS news_item (
                id              INTEGER PRIMARY KEY,
                channel         TEXT NOT NULL,
                title           TEXT NOT NULL,
                link            TEXT NOT NULL UNIQUE,
                description     TEXT NOT NULL,
                creators        TEXT,
                pub_date        TEXT,
                categories      TEXT,
                keywords        TEXT,
                clean_content   TEXT,
                error           TEXT
            )",
        )?;
        Ok(())
    }

    pub fn insert(&self, conn: &Connection) -> sqlite::Result<()> {
        let mut statement = conn.prepare(
            "INSERT INTO news_item (channel, title, link, description, creators, pub_date, categories, keywords, clean_content, error) 
             VALUES (:channel, :title, :link, :description, :creators, :pub_date, :categories, :keywords, :clean_content, :error)",
        )?;
        // Bind the values
        statement.bind(&self.binds()[..])?;

        statement.next()?; // Execute the statement
        Ok(())
    }

    pub fn query_all(conn: &Connection) -> sqlite::Result<Vec<NewsItem>> {
        let mut statement = conn.prepare(
            "SELECT channel, title, link, description, creators, pub_date, categories, keywords, clean_content, error FROM news_item",
        )?;

        let mut news_items = Vec::new();
        while let State::Row = statement.next()? {
            let channel: String = statement.read(0)?;
            let title: String = statement.read(1)?;
            let link: String = statement.read(2)?;
            let description: String = statement.read(3)?;
            let creators: String = statement.read(4)?;
            let pub_date: Option<String> = statement.read::<Option<String>, _>(5)?;
            let categories: Option<String> = statement.read::<Option<String>, _>(6)?;
            let keywords: Option<String> = statement.read::<Option<String>, _>(7)?;
            let clean_content: Option<String> = statement.read::<Option<String>, _>(8)?;
            let error: Option<String> = statement.read::<Option<String>, _>(9)?;

            let error = match error {
                Some(e) => match e.as_str() {
                    "None" => None,
                    _ => match PipelineError::from_str(&e) {
                        Ok(e) => Some(e),
                        Err(_) => Some(PipelineError::UnknownError),
                    },
                },
                None => None,
            };

            news_items.push(NewsItem {
                channel,
                title,
                link,
                description,
                creators,
                pub_date,
                categories,
                keywords,
                clean_content,
                error,
                relevance: None,
            });
        }
        Ok(news_items)
    }
}

/// Function that writes a slice of items to a CSV file
pub fn write_news_items_to_csv(news_items: &[NewsItem], file: &str) -> anyhow::Result<()> {
    // Create a CSV writer that writes to the given file
    let file = File::create(file)?;
    let mut writer = Writer::from_writer(file);

    // Write the header row
    writer.write_record(&[
        "Channel", "Title", "Link", "Description", "Creators", 
        "Publication Date", "Categories", "Keywords", "Clean Content", 
        "Error", "Feedback Date", "Relevance", "V1", "V2",
    ])?;

    // Feedback date with format: Sun, 01 Jan 2017 12:00:00 +0000
    let feedback_date = chrono::Utc::now().to_rfc2822();

    // Iterate over each NewsItem and write its fields to the CSV
    for item in news_items {
        writer.write_record(&[
            &item.channel,
            &item.title,
            &item.link,
            &item.description,
            &item.creators,
            item.pub_date.as_deref().unwrap_or(""),
            item.categories.as_deref().unwrap_or(""),
            item.keywords.as_deref().unwrap_or(""),
            item.clean_content.as_deref().unwrap_or(""),
            &format!("{:?}", item.error),
            &feedback_date,
            &item.relevance.map_or(String::new(), |r| r.to_string()),
        ])?;
    }

    // Flush and finish writing
    writer.flush()?;
    Ok(())
}

// Function to convert FeedbackRecords to Arrow arrays and write them as Parquet
// TODO: manage f64 and f32
pub fn write_feedback_records_parquet(records: Vec<FeedbackRecord>, file: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Extract relevant fields from FeedbackRecords
    let mut channels = vec![];
    let mut titles = vec![];
    let mut creators = vec![];
    let mut pub_dates = vec![];
    let mut categories = vec![];
    let mut keywords = vec![];
    let mut relevances = vec![];
    let mut title_embeddings_flattened = vec![];
    let mut keywords_and_categories_embeddings_flattened = vec![];

    // Assert that the title and keyword and catergories embeddings have the same size
    assert!(records[0].title_embedding.len() == records[0].keywords_and_categories_embedding.len());

    let embeddings_list_size = records[0].title_embedding.len();

    // Assert that the keywords and categories embeddings have the same size
    assert!(records.iter().all(|record| record.title_embedding.len() == embeddings_list_size));
    assert!(records.iter().all(|record| record.keywords_and_categories_embedding.len() == embeddings_list_size));

    // Iterate over the records and extract the fields
    for record in records.iter() {
        channels.push(record.news_item.channel.clone());
        titles.push(record.news_item.title.clone());
        creators.push(record.news_item.creators.clone());
        pub_dates.push(record.news_item.pub_date.clone().unwrap_or_default());
        categories.push(record.news_item.categories.clone().unwrap_or_default());
        keywords.push(record.news_item.keywords.clone().unwrap_or_default());
        relevances.push(record.news_item.relevance.unwrap_or_default() as f32); // Handle relevance as f32
        
        // Handle embeddings (flatten Option<Vec<f32>> to Vec<f32>)
        title_embeddings_flattened.extend(record.title_embedding.clone());
        keywords_and_categories_embeddings_flattened.extend(record.keywords_and_categories_embedding.clone());
        
        // Print length of all columns
        println!("Channels: {}, Titles: {}, Creators: {}, Pub Dates: {}, Categories: {}, Keywords: {}, Relevances: {}, Title Embeddings: {}, Keywords and Categories Embeddings: {}", 
            channels.len(), titles.len(), creators.len(), pub_dates.len(), categories.len(), keywords.len(), relevances.len(), title_embeddings_flattened.len(), keywords_and_categories_embeddings_flattened.len());
    }

    // Create Arrow arrays
    let channel_array = StringArray::from(channels);
    let title_array = StringArray::from(titles);
    let creator_array = StringArray::from(creators);
    let pub_date_array = StringArray::from(pub_dates);
    let categories_array = StringArray::from(categories);
    let keywords_array = StringArray::from(keywords);
    let relevance_array = arrow::array::Float32Array::from(relevances);

    // ----- Title Embedding column: FixedSizeList of f32 -----
    let (title_embeddings_array, title_embeddings_array_type) = create_fixed_size_list_array_of_floats(&title_embeddings_flattened, embeddings_list_size as i32, records.len());
    
    // ----- Keywords and Categories Embedding column: FixedSizeList of f32 -----
    let (keywords_and_categories_embeddings_array, keywords_and_categories_embeddings_array_type) = create_fixed_size_list_array_of_floats(&keywords_and_categories_embeddings_flattened, embeddings_list_size as i32, records.len());


    // Create the schema for the RecordBatch
    let schema = Arc::new(Schema::new(vec![
        Field::new("channel", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("creators", DataType::Utf8, false),
        Field::new("pub_date", DataType::Utf8, false),
        Field::new("categories", DataType::Utf8, false),
        Field::new("keywords", DataType::Utf8, false),
        Field::new("relevance", DataType::Float32, false),
        Field::new("title_embedding", title_embeddings_array_type.clone(), true),
        Field::new("keywords_and_categories_embedding", keywords_and_categories_embeddings_array_type.clone(), true),
    ]));

    // Create a RecordBatch from the arrays
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(channel_array) as ArrayRef,
            Arc::new(title_array) as ArrayRef,
            Arc::new(creator_array) as ArrayRef,
            Arc::new(pub_date_array) as ArrayRef,
            Arc::new(categories_array) as ArrayRef,
            Arc::new(keywords_array) as ArrayRef,
            Arc::new(relevance_array) as ArrayRef,
            Arc::new(title_embeddings_array) as ArrayRef,
            Arc::new(keywords_and_categories_embeddings_array) as ArrayRef,
        ],
    )?;

    // Create a file to write the Parquet data
    let file = File::create(file)?;

    // Set writer properties (optional)
    let writer_props = WriterProperties::builder()
        .set_compression(parquet::basic::Compression::SNAPPY)
        .build();

    // Create the ArrowWriter
    let mut writer = ArrowWriter::try_new(file, schema, Some(writer_props))?;

    // Write the RecordBatch
    writer.write(&batch)?;

    // Close the writer to finalize the file
    writer.close()?;

    Ok(())
}

/// Function to create a FixedSizeListArray from a flattened array of f32 values
fn create_fixed_size_list_array_of_floats(embeddings_flattened: &[f32], list_size: i32, number_of_lists: usize) -> (FixedSizeListArray, DataType) {
    // Construct the value array (the underlying Float32 data)
    let embeddings_data = ArrayData::builder(DataType::Float32)
        .len(embeddings_flattened.len())  // Total length of the flat data
        .add_buffer(Buffer::from_slice_ref(&embeddings_flattened))  // Buffer of f32 values
        .build()
        .unwrap();

    // Define a FixedSizeList of Float32 type, where each list has exactly list_size items
    let list_data_type = DataType::FixedSizeList(
        Arc::new(Field::new("item", DataType::Float32, false)),  // Each list contains Float32 values
        list_size,  // Each list contains list_size elements
    );

    // Construct the FixedSizeListArray with 3 lists
    let list_data = ArrayData::builder(list_data_type.clone())
        .len(number_of_lists)  // Number of lists (3 lists: one for each name)
        .add_child_data(embeddings_data)  // Pass the underlying f32 value data
        .build()
        .unwrap();

    let list_array = FixedSizeListArray::from(list_data);

    (list_array, list_data_type)
}