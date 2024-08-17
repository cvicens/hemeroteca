use serde_json::json;

/// Module for OpenAI related functions

/// Function that given a text returns a summary
pub async fn summarize(text: &str, api_key: &str) -> Result<String, reqwest::Error> {
    let client = reqwest::Client::new();

    // Create the JSON payload for the API request
    let payload = json!({
        "model": "text-davinci-003", // Specify the model you want to use
        "prompt": format!("Summarize the following text: {}", text),
        "max_tokens": 150, // Adjust the max tokens as needed
        "temperature": 0.7 // Adjust the temperature as needed
    });

    let response = client
        .post("https://api.openai.com/v1/completions")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&payload)
        .send()
        .await?;
    let response = response.text().await?;
    Ok(response)
}
