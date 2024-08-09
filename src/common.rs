/// Common types and utilities used across the library

use std::error::Error;

/// Enum that represents the different types of channels
#[derive(Debug, PartialEq, Eq)]
pub enum ChannelType {
    ElPais,
    VeinteMinutos,
    ElDiario,
    Other,
}

/// Struct that represents a News Item
#[derive(Debug, Clone)]
pub struct NewsItem {
    pub channel: String,
    pub title: String,
    pub link: String,
    pub description: String,
    pub pub_date: Option<String>,
    pub categories: Option<String>,
    pub keywords: Option<String>,
    pub clean_content: Option<String>,
    pub error: Option<PipelineError>,
}

// Define a custom error type for the pipeline
#[derive(Debug, Clone)]
pub enum PipelineError {
    EmptyString,
    ParsingError(String),
    NoContent,
    NetworkError(String),
    UnknownError,
}

// Default implementation for the PipelineError enum
impl Default for PipelineError {
    fn default() -> Self {
        PipelineError::NoContent
    }
}

// Return a str representation of the PipelineError
impl PipelineError {
    /// Function that returns PipelineError from a &str
    pub fn from_str(error: &str) -> Result<Self, Box<dyn Error>> {
        // Match error by using a regex pattern: 
        // - ParsingError(.*) => ParsingError 
        // - NetworkError(.*) => NetworkError
        // - EmptyString
        // - NoContent
        let re = regex::Regex::new(r"^(ParsingError\((.*)\)|NetworkError\((.*)\)|EmptyString|NoContent)$")?;
        let caps = re.captures(error).ok_or("No match")?;
        match caps.get(1).map(|m| m.as_str()) {
            Some("EmptyString") => Ok(PipelineError::EmptyString),
            Some("NoContent") => Ok(PipelineError::NoContent),
            Some("ParsingError") => {
                let msg = caps.get(2).ok_or("No match")?.as_str().to_string();
                Ok(PipelineError::ParsingError(msg))
            }
            Some("NetworkError") => {
                let msg = caps.get(3).ok_or("No match")?.as_str().to_string();
                Ok(PipelineError::NetworkError(msg))
            }
            _ => Err("No match".into()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            PipelineError::EmptyString => "EmptyString",
            PipelineError::ParsingError(_) => "ParsingError",
            PipelineError::NoContent => "NoContent",
            PipelineError::NetworkError(_) => "NetworkError",
            PipelineError::UnknownError => "UnknownError",
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            PipelineError::EmptyString => "EmptyString".to_string(),
            PipelineError::ParsingError(e) => format!("ParsingError({})", e),
            PipelineError::NoContent => "NoContent".to_string(),
            PipelineError::NetworkError(e) => format!("NetworkError({})", e),
            PipelineError::UnknownError => "UnknownError".to_string(),
        }
    }
}

impl NewsItem {
    /// Function that creates a NewsItem from an RSS Item and returns a Result or Error
    ///
    /// Example:
    /// ```
    /// use rss::Item;
    /// use hemeroteca::prelude::*;
    ///
    /// let item = Item::default();
    /// let news_item = NewsItem::from_item("Other", &item);
    /// assert_eq!(news_item.is_err(), true);
    /// ```
    pub fn from_item(channel: &str, item: &rss::Item) -> Result<NewsItem, Box<dyn Error>> {
        let title = item.title().ok_or("No title")?.to_string();
        let link = item.link().ok_or("No link")?.to_string();
        let description = item.description().ok_or("No description")?.to_string();
        let pub_date = item.pub_date().map(|date| date.to_string());
        let categories = item
            .categories()
            .iter()
            .map(|category| category.name().to_string().to_lowercase())
            .collect::<Vec<String>>();
        let categories = if categories.is_empty() {
            None
        } else {
            Some(categories.join(","))
        };
        let extensions = item.extensions().clone();
        let keywords = extensions.get("media").and_then(|ext| {
            ext.get("keywords")
                .and_then(|extensions| {
                    extensions
                        .iter()
                        .map(|ext| {
                            if ext.name == "media:keywords" && ext.value.is_some() {
                                Some(ext.value().unwrap().to_string().to_lowercase())
                            } else {
                                None
                            }
                        })
                        .collect::<Option<Vec<String>>>()
                })
                .map(|keywords| keywords.join(","))
        });
        Ok(NewsItem {
            channel: channel.to_string(),
            title,
            link,
            description,
            pub_date,
            categories,
            keywords,
            clean_content: None,
            error: None,
        })
    }
}

// /// Function that returns a String given an Option<PipelineError>
// /// 
// /// Example:
// /// ```
// /// use hemeroteca::prelude::*;
// /// 
// /// let error = PipelineError::EmptyString;
// /// let error_str = error_to_string(&error);
// /// assert_eq!(error_str, "EmptyString");
// /// ```
// pub fn error_to_string(error: &Option<PipelineError>) -> String {
//     match error {
//         Some(PipelineError::EmptyString) => "EmptyString".to_string(),
//         Some(PipelineError::ParsingError(msg)) => format!("ParsingError: {}", msg),
//         Some(PipelineError::NoContent) => "NoContent".to_string(),
//         Some(PipelineError::NetworkError(msg)) => format!("NetworkError: {}", msg),
//         Some(PipelineError::UnknownError) => "UnknownError".to_string(),
//         None => "None".to_string(),
//     }
// }