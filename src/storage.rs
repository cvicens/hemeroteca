/// Module for storage related functions
use crate::common::{FeedbackRecord, NewsItem, PipelineError};

use arrow::buffer::Buffer;
use arrow::compute::concat;
use csv::Writer;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::Compression;

use sqlite::{Connection, State};

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use arrow::array::{ArrayData, ArrayRef, FixedSizeListArray, StringArray};
use arrow::datatypes::{DataType, Field, Schema, SchemaRef};
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

/// Function that writes a slice of items to a CSV file. If the file already exists, it will add
/// the items to the end of the file with no header.
pub fn write_feedback_records_to_csv(records: &Vec<FeedbackRecord>, file: &str) -> anyhow::Result<()> {
    // If the file already exists, open it in append mode
    let mut writer = if std::path::Path::new(file).exists() {
        // Open the file in append mode
        let file = OpenOptions::new().append(true).open(file)?;
        let mut writer = Writer::from_writer(file);

        writer.flush()?;
        writer
    } else {
        // Create a CSV writer that writes to the given file
        let file = File::create(file)?;
        let mut writer = Writer::from_writer(file);

        // Write the header row
        writer.write_record(&[
            "Channel", "Title", "Link", "Description", "Creators", 
            "Publication Date", "Categories", "Keywords", "Clean Content", 
            "Error", "Feedback Date", "Relevance", "Title Embedding", "Keywords and Categories Embedding",
        ])?;
        writer
    };

    // Feedback date with format: Sun, 01 Jan 2017 12:00:00 +0000
    let feedback_date = chrono::Utc::now().to_rfc2822();

    // Iterate over each FeedbackRecord and write its fields to the CSV
    for record in records {
        writer.write_record(&[
            &record.news_item.channel,
            &record.news_item.title,
            &record.news_item.link,
            &record.news_item.description,
            &record.news_item.creators,
            record.news_item.pub_date.as_deref().unwrap_or(""),
            record.news_item.categories.as_deref().unwrap_or(""),
            record.news_item.keywords.as_deref().unwrap_or(""),
            record.news_item.clean_content.as_deref().unwrap_or(""),
            &format!("{:?}", record.news_item.error),
            &feedback_date,
            &record.news_item.relevance.map_or(String::new(), |r| r.to_string()),
            &record.title_embedding.iter().map(|v| v.to_string()).collect::<Vec<String>>().join(","),
            &record.bow_embedding.iter().map(|v| v.to_string()).collect::<Vec<String>>().join(","),
        ])?;
    }

    // Flush and finish writing
    writer.flush()?;
    Ok(())
}

/// Function to concatenate multiple RecordBatches into a single RecordBatch
pub fn concat_batches(schema: &SchemaRef, batches: &[RecordBatch]) -> arrow::error::Result<RecordBatch> {
    if batches.is_empty() {
        return Err(arrow::error::ArrowError::InvalidArgumentError(
            "No record batches to concatenate".to_string(),
        ));
    }

    // Concatenate the arrays column by column
    let columns: Vec<ArrayRef> = (0..batches[0].num_columns())
        .map(|i| {
            let arrays: Vec<&dyn arrow::array::Array> = batches
                .iter()
                .map(|batch| batch.column(i).as_ref())
                .collect();
            concat(&arrays)
        })
        .collect::<arrow::error::Result<Vec<_>>>()?;

    // Create a new RecordBatch with the concatenated columns
    RecordBatch::try_new(schema.clone(), columns)
}

// Function to convert FeedbackRecords to Arrow arrays and write them as Parquet
// TODO: manage f64 and f32
pub fn write_feedback_records_parquet(records: &Vec<FeedbackRecord>, file: &str) -> anyhow::Result<()> {
    // Extract relevant fields from FeedbackRecords
    let mut channels = vec![];
    let mut titles = vec![];
    let mut links = vec![];
    let mut descriptions = vec![];
    let mut creators = vec![];
    let mut pub_dates = vec![];
    let mut categories = vec![];
    let mut keywords = vec![];
    let mut contents = vec![];
    let mut relevances = vec![];
    let mut title_embeddings_flattened = vec![];
    let mut keywords_and_categories_embeddings_flattened = vec![];

    // Assert that the title and keyword and catergories embeddings have the same size
    assert!(records[0].title_embedding.len() == records[0].bow_embedding.len());

    let embeddings_list_size = records[0].title_embedding.len();

    // Assert that the keywords and categories embeddings have the same size
    assert!(records.iter().all(|record| record.title_embedding.len() == embeddings_list_size));
    assert!(records.iter().all(|record| record.bow_embedding.len() == embeddings_list_size));

    // Iterate over the records and extract the fields
    for record in records.iter() {
        channels.push(record.news_item.channel.clone());
        titles.push(record.news_item.title.clone());
        links.push(record.news_item.link.clone());
        descriptions.push(record.news_item.description.clone());
        creators.push(record.news_item.creators.clone());
        pub_dates.push(record.news_item.pub_date.clone().unwrap_or_default());
        categories.push(record.news_item.categories.clone().unwrap_or_default());
        keywords.push(record.news_item.keywords.clone().unwrap_or_default());
        contents.push(record.news_item.clean_content.clone().unwrap_or_default());
        relevances.push(record.news_item.relevance.unwrap_or_default());
        
        // Handle embeddings (flatten Option<Vec<f32>> to Vec<f32>)
        title_embeddings_flattened.extend(record.title_embedding.clone());
        keywords_and_categories_embeddings_flattened.extend(record.bow_embedding.clone());
        
        // Print length of all columns
        log::debug!("Channels: {}, Titles: {}, Creators: {}, Pub Dates: {}, Categories: {}, Keywords: {}, Relevances: {}, Title Embeddings: {}, Keywords and Categories Embeddings: {}", 
            channels.len(), titles.len(), creators.len(), pub_dates.len(), categories.len(), keywords.len(), relevances.len(), title_embeddings_flattened.len(), keywords_and_categories_embeddings_flattened.len());
    }

    // Create Arrow arrays
    let channel_array = StringArray::from(channels);
    let title_array = StringArray::from(titles);
    let link_array = StringArray::from(links);
    let description_array = StringArray::from(descriptions);
    let creator_array = StringArray::from(creators);
    let pub_date_array = StringArray::from(pub_dates);
    let categories_array = StringArray::from(categories);
    let keywords_array = StringArray::from(keywords);
    let content_array = StringArray::from(contents);
    let relevance_array = arrow::array::Float64Array::from(relevances);

    // ----- Title Embedding column: FixedSizeList of f32 -----
    let (title_embeddings_array, title_embeddings_array_type) = create_fixed_size_list_array_of_floats(&title_embeddings_flattened, embeddings_list_size as i32, records.len());
    
    // ----- Keywords and Categories Embedding column: FixedSizeList of f32 -----
    let (keywords_and_categories_embeddings_array, keywords_and_categories_embeddings_array_type) = create_fixed_size_list_array_of_floats(&keywords_and_categories_embeddings_flattened, embeddings_list_size as i32, records.len());


    // Create the schema for the RecordBatch
    let schema = Arc::new(Schema::new(vec![
        Field::new("channel", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("link", DataType::Utf8, false),
        Field::new("description", DataType::Utf8, false),
        Field::new("creators", DataType::Utf8, false),
        Field::new("pub_date", DataType::Utf8, false),
        Field::new("categories", DataType::Utf8, false),
        Field::new("keywords", DataType::Utf8, false),
        Field::new("content", DataType::Utf8, false),
        Field::new("relevance", DataType::Float64, false),
        Field::new("title_embedding", title_embeddings_array_type.clone(), true),
        Field::new("keywords_and_categories_embedding", keywords_and_categories_embeddings_array_type.clone(), true),
    ]));

    // Create a RecordBatch from the arrays
    let new_batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(channel_array) as ArrayRef,
            Arc::new(title_array) as ArrayRef,
            Arc::new(link_array) as ArrayRef,
            Arc::new(description_array) as ArrayRef,
            Arc::new(creator_array) as ArrayRef,
            Arc::new(pub_date_array) as ArrayRef,
            Arc::new(categories_array) as ArrayRef,
            Arc::new(keywords_array) as ArrayRef,
            Arc::new(content_array) as ArrayRef,
            Arc::new(relevance_array) as ArrayRef,
            Arc::new(title_embeddings_array) as ArrayRef,
            Arc::new(keywords_and_categories_embeddings_array) as ArrayRef,
        ],
    )?;

    let path = Path::new(file);
    let combined_batch: RecordBatch;

    if path.exists() {
        // File exists, open and read the existing Parquet file
        let file = File::open(file)?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
        log::debug!("Converted arrow schema is: {}", builder.schema());

        let reader = builder.build()?;

        // Iterate the reader to get the existing batches, print the number of rows or errors and return a Vec<RecordBatch>
        let mut existing_batches = reader.into_iter().filter_map(|result| match result {
            Ok(batch) => {
                log::debug!("Read {} records.", batch.num_rows());
                Some(batch)
            }
            Err(e) => {
                log::debug!("Error reading batch: {}", e);
                None
            }
        })
        .filter_map(Option::Some)
        .collect::<Vec<RecordBatch>>();
        

        // Add the existing batches to the new batch
        existing_batches.push(new_batch);

        // Concatenate existing batches with the new batch
        combined_batch = concat_batches(&schema, &existing_batches)?;

    } else {
        // File does not exist, use the new batch as is
        combined_batch = new_batch;
    }

    // Open the file in append mode, create if not exists
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true) // Overwrite content
        .open(file)?;

    // Set writer properties
    let writer_props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();

    // Create the ArrowWriter
    let mut writer = ArrowWriter::try_new(file, schema, Some(writer_props))?;

    // Write the combined RecordBatch
    writer.write(&combined_batch)?;

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

    // Define a FixedSizeList of Float64 type, where each list has exactly list_size items
    let list_data_type = DataType::FixedSizeList(
        Arc::new(Field::new("item", DataType::Float32, false)),  // Each list contains f32 values
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

/// Function that reads feedback records from a parquet file
/// 
/// Example:
/// 
/// ```rust
/// use hemeroteca::prelude::read_feedback_records_from_parquet;
/// 
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// 
/// use hemeroteca::prelude::*;
/// use std::fs::remove_file;
/// 
/// let records = vec![
/// FeedbackRecord {
///     news_item: NewsItem::default(Some(1.0)),
///     title_embedding: vec![1.0, 2.0, 3.0],
///     bow_embedding: vec![4.0, 5.0, 6.0],
/// },
/// FeedbackRecord {
///     news_item: NewsItem::default(Some(4.0)),
///     title_embedding: vec![7.0, 8.0, 9.0],
///     bow_embedding: vec![10.0, 11.0, 12.0],
/// },
/// ];
/// 
/// let file = "test.parquet";
/// let result = write_feedback_records_parquet(&records, file);
/// assert_eq!(result.is_ok(), true);
/// 
/// let feedback_records = read_feedback_records_from_parquet(file).await.unwrap();
/// assert_eq!(feedback_records.len(), 2);
/// 
/// // Clean up
/// remove_file(file).unwrap();
/// # }
/// ```
pub async fn read_feedback_records_from_parquet(file: &str) -> anyhow::Result<Vec<FeedbackRecord>> {
    // If file does not exist, return an error
    if !Path::new(file).exists() {
        return Err(anyhow::anyhow!("File does not exist."));
    }
    // Open the file
    let file = File::open(file)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    log::debug!("Converted arrow schema is: {}", builder.schema());

    let reader = builder.build()?;

    // Iterate the reader to get the existing batches, print the number of rows or errors and return a Vec<RecordBatch>
    let feedback_records = reader.into_iter().filter_map(|result| match result {
        Ok(batch) => {
            log::debug!("Read {} records.", batch.num_rows());
            Some(batch)
        }
        Err(e) => {
            log::debug!("Error reading batch: {}", e);
            None
        }
    })
    .filter_map(|batch| {
        let channel = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        let title = batch.column(1).as_any().downcast_ref::<StringArray>().unwrap();
        let link = batch.column(2).as_any().downcast_ref::<StringArray>().unwrap();
        let description = batch.column(3).as_any().downcast_ref::<StringArray>().unwrap();
        let creators = batch.column(4).as_any().downcast_ref::<StringArray>().unwrap();
        let pub_date = batch.column(5).as_any().downcast_ref::<StringArray>().unwrap();
        let categories = batch.column(6).as_any().downcast_ref::<StringArray>().unwrap();
        let keywords = batch.column(7).as_any().downcast_ref::<StringArray>().unwrap();
        let content = batch.column(8).as_any().downcast_ref::<StringArray>().unwrap();
        let relevance = batch.column(9).as_any().downcast_ref::<arrow::array::Float64Array>().unwrap();
        let title_embeddings = batch.column(10).as_any().downcast_ref::<FixedSizeListArray>().unwrap();
        let keywords_and_categories_embeddings = batch.column(11).as_any().downcast_ref::<FixedSizeListArray>().unwrap();

        let feedback_records = (0..batch.num_rows()).map(|i| {
            let title_embedding = title_embeddings.value(i).as_any().downcast_ref::<arrow::array::Float32Array>().unwrap().values().to_vec();
            let keywords_and_categories_embedding = keywords_and_categories_embeddings.value(i).as_any().downcast_ref::<arrow::array::Float32Array>().unwrap().values().to_vec();

            FeedbackRecord {
                news_item: NewsItem {
                    channel: channel.value(i).to_string(),
                    title: title.value(i).to_string(),
                    link: link.value(i).to_string(),
                    description: description.value(i).to_string(),
                    creators: creators.value(i).to_string(),
                    pub_date: Some(pub_date.value(i).to_string()),
                    categories: Some(categories.value(i).to_string()),
                    keywords: Some(keywords.value(i).to_string()),
                    relevance: Some(relevance.value(i)),
                    clean_content: Some(content.value(i).to_string()),
                    error: None,
                },
                title_embedding,
                bow_embedding: keywords_and_categories_embedding,
            }
        }).collect::<Vec<FeedbackRecord>>();

        Some(feedback_records)
    })
    .flatten()
    .collect::<Vec<FeedbackRecord>>();

    Ok(feedback_records)
}

mod test {
    
    #[test]
    fn test_news_item_binds() {
        use super::NewsItem;

        let news_item = NewsItem {
            channel: "Channel".to_string(),
            title: "Title".to_string(),
            link: "Link".to_string(),
            description: "Description".to_string(),
            pub_date: Some("Date".to_string()),
            categories: Some("Categories".to_string()),
            keywords: Some("Keywords".to_string()),
            clean_content: Some("Clean Content".to_string()),
            creators: "John Doe".to_string(),
            error: None,
            relevance: None,
        };

        let binds = news_item.binds();
        assert_eq!(binds.len(), 10);
    }

    #[test]
    fn test_write_feedback_records_to_csv() {
        use super::{FeedbackRecord, NewsItem};
        use std::fs::remove_file;

        let records = vec![
            FeedbackRecord {
                news_item: NewsItem::default(Some(4.0)),
                title_embedding: vec![1.0, 2.0, 3.0],
                bow_embedding: vec![4.0, 5.0, 6.0],
            },
            FeedbackRecord {
                news_item: NewsItem::default(Some(4.0)),
                title_embedding: vec![7.0, 8.0, 9.0],
                bow_embedding: vec![10.0, 11.0, 12.0],
            },
        ];

        let file = "test-write.csv";
        let result = super::write_feedback_records_to_csv(&records, file);
        assert_eq!(result.is_ok(), true);

        // Clean up
        remove_file(file).unwrap();
    }

    #[test]
    fn test_write_feedback_records_parquet() {
        use super::{FeedbackRecord, NewsItem};
        use std::fs::remove_file;

        let records = vec![
            FeedbackRecord {
                news_item: NewsItem::default(Some(4.0)),
                title_embedding: vec![1.0, 2.0, 3.0],
                bow_embedding: vec![4.0, 5.0, 6.0],
            },
            FeedbackRecord {
                news_item: NewsItem::default(Some(5.0)),
                title_embedding: vec![7.0, 8.0, 9.0],
                bow_embedding: vec![10.0, 11.0, 12.0],
            },
        ];

        let file = "test-write.parquet";
        let result = super::write_feedback_records_parquet(&records, file);
        assert_eq!(result.is_ok(), true);

        // Clean up
        remove_file(file).unwrap();
    }

    #[tokio::test]
    async fn test_read_feedback_records_from_parquet() {
        use super::{FeedbackRecord, NewsItem};
        use std::fs::remove_file;

        let records = vec![
            FeedbackRecord {
                news_item: NewsItem::default(Some(1.0)),
                title_embedding: vec![1.0, 2.0, 3.0],
                bow_embedding: vec![4.0, 5.0, 6.0],
            },
            FeedbackRecord {
                news_item: NewsItem::default(Some(4.0)),
                title_embedding: vec![7.0, 8.0, 9.0],
                bow_embedding: vec![10.0, 11.0, 12.0],
            },
        ];

        let file = "test-read.parquet";
        let result = super::write_feedback_records_parquet(&records, file);
        assert_eq!(result.is_ok(), true);

        let feedback_records = super::read_feedback_records_from_parquet(file).await.unwrap();
        assert_eq!(feedback_records.len(), 2);

        // Clean up
        remove_file(file).unwrap();
    }
}