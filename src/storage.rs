/// Module for storage related functions
/// 

use crate::common::{NewsItem, PipelineError};

use sqlite::{Connection, State};

impl NewsItem {

    /// Function that returns a Bindable slice of tuples with the values of the NewsItem
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
    ///    error: None,
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
            let clean_content: Option<String> = statement.read::<Option<String>, _ >(8)?;
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
            });
        }
        Ok(news_items)
    }
}