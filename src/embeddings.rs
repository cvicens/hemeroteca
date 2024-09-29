//! Library that provides functions for embedding text using a pre-trained transformer model.

use candle_core::{CudaDevice, Device, Tensor, WithDType};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, HiddenAct, DTYPE};
use hf_hub::{api::sync::Api, Repo, RepoType};
use tokenizers::{PaddingParams, Tokenizer};

// Default model id and revision
pub const DEFAULT_MODEL_ID: &str = "sentence-transformers/all-MiniLM-L6-v2";
pub const DEFAULT_REVISION: &str = "refs/pr/21";

pub fn normalize_l2(v: &Tensor) -> anyhow::Result<Tensor> {
    Ok(v.broadcast_div(&v.sqr()?.sum_keepdim(1)?.sqrt()?)?)
}

pub trait FloatType {
    fn sqrt(self) -> Self;    
}

impl FloatType for f32 {
    fn sqrt(self) -> Self {
        self.sqrt()
    }
}

impl FloatType for f64 {
    fn sqrt(self) -> Self {
        self.sqrt()
    }
}

pub async fn cosine_similarity<T: FloatType + WithDType>(a: Tensor, b: Tensor) -> anyhow::Result<T> {
    let sum_ab = (&a * &b)?.sum_all()?.to_scalar::<T>()?;
    let sum_a2 = (&a * &a)?.sum_all()?.to_scalar::<T>()?;
    let sum_b2 = (&b * &b)?.sum_all()?.to_scalar::<T>()?;
    Ok(sum_ab / (sum_a2 * sum_b2).sqrt())
}

pub async fn generate_embeddings(mut tokenizer: Tokenizer, model: BertModel, sentences: &Vec<&str>, normalize_embeddings: bool) -> anyhow::Result<Tensor> {
    if let Some(pp) = tokenizer.get_padding_mut() {
        pp.strategy = tokenizers::PaddingStrategy::BatchLongest
    } else {
        let pp = PaddingParams {
            strategy: tokenizers::PaddingStrategy::BatchLongest,
            ..Default::default()
        };
        tokenizer.with_padding(Some(pp));
    }
    let tokens = tokenizer
        .encode_batch(sentences.to_vec(), true)
        .map_err(anyhow::Error::msg)?;
    let token_ids = tokens
        .iter()
        .map(|tokens| {
            let tokens = tokens.get_ids().to_vec();
            Ok(Tensor::new(tokens.as_slice(), &model.device)?)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let attention_mask = tokens
        .iter()
        .map(|tokens| {
            let tokens = tokens.get_attention_mask().to_vec();
            Ok(Tensor::new(tokens.as_slice(), &model.device)?)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let token_ids = Tensor::stack(&token_ids, 0)?;
    let _attention_mask = Tensor::stack(&attention_mask, 0)?;
    let token_type_ids = token_ids.zeros_like()?;
    log::trace!("running inference on batch {:?}", token_ids.shape());
    let embeddings = model.forward(&token_ids, &token_type_ids)?;
    log::trace!("generated embeddings {:?}", embeddings.shape());
    // Apply some avg-pooling by taking the mean embedding value for all tokens (including padding)
    let (_n_sentence, n_tokens, _hidden_size) = embeddings.dims3()?;
    let embeddings = (embeddings.sum(1)? / (n_tokens as f64))?;
    if normalize_embeddings {
        Ok(normalize_l2(&embeddings)?)
    } else {
        Ok(embeddings)
    }
}

pub async fn generate_embedding(mut tokenizer: Tokenizer, model: BertModel, prompt: String, normalize_embedding: bool) -> anyhow::Result<Tensor> {
    let tokenizer = tokenizer
            .with_padding(None)
            .with_truncation(None)
            .map_err(anyhow::Error::msg)?;
    let tokens = tokenizer
        .encode(prompt, true)
        .map_err(anyhow::Error::msg)?
        .get_ids()
        .to_vec();
    let token_ids = Tensor::new(&tokens[..], &model.device)?.unsqueeze(0)?;
    let token_type_ids = token_ids.zeros_like()?;

    let embedding = model.forward(&token_ids, &token_type_ids)?;

    if normalize_embedding {
        Ok(normalize_l2(&embedding)?)
    } else {
        Ok(embedding)
    }
}

/// Build a model and tokenizer from a model id, revision, and other options.
pub fn build_model_and_tokenizer(model_id: &str, revision: &str, gpu: bool, use_pth: bool, approximate_gelu: bool) -> anyhow::Result<(BertModel, Tokenizer)> {
    let device = if gpu {
        Device::Cuda(CudaDevice)
    } else {
        Device::Cpu
    };

    let repo = Repo::with_revision(model_id.to_owned(), RepoType::Model, revision.to_owned());
    let (config_filename, tokenizer_filename, weights_filename) = {
        let api = Api::new()?;
        let api = api.repo(repo);
        let config = api.get("config.json")?;
        let tokenizer = api.get("tokenizer.json")?;
        let weights = if use_pth {
            api.get("pytorch_model.bin")?
        } else {
            api.get("model.safetensors")?
        };
        (config, tokenizer, weights)
    };
    let config = std::fs::read_to_string(config_filename)?;
    let mut config: Config = serde_json::from_str(&config)?;
    let tokenizer = Tokenizer::from_file(tokenizer_filename).map_err(anyhow::Error::msg)?;

    let vb = if use_pth {
        VarBuilder::from_pth(&weights_filename, DTYPE, &device)?
    } else {
        unsafe { VarBuilder::from_mmaped_safetensors(&[weights_filename], DTYPE, &device)? }
    };
    if approximate_gelu {
        config.hidden_act = HiddenAct::GeluApproximate;
    }
    let model = BertModel::load(vb, &config)?;
    Ok((model, tokenizer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::Shape;

    #[tokio::test]
    async fn test_generate_embeddings() -> anyhow::Result<()> {
        let model_id = DEFAULT_MODEL_ID.to_string();
        let revision = DEFAULT_REVISION.to_string();
        let gpu = false;
        let use_pth = false;
        let approximate_gelu = false;
        let (model, tokenizer) = build_model_and_tokenizer(&model_id, &revision, gpu, use_pth, approximate_gelu)?;
        let sentences = vec!["Hello, my dog is cute.", "Hello, my cat is cute."];
        let embeddings = generate_embeddings(tokenizer, model, &sentences, true).await?;
        // Print embeddings
        println!("embeddings {:?}", embeddings);

        // Extract the embedding of the first sentence
        let first_embedding = embeddings.narrow(0, 0, 1)?; // Narrow along the first dimension (0), selecting index 0 with length 1
        println!("first_embedding {:?}", first_embedding);


        assert_eq!(*embeddings.shape(), Shape::from(&[2, 384]));
        Ok(())
    }

    #[tokio::test]
    async fn test_generate_embedding() -> anyhow::Result<()> {
        let model_id = DEFAULT_MODEL_ID.to_string();
        let revision = DEFAULT_REVISION.to_string();
        let gpu = false;
        let use_pth = false;
        let approximate_gelu = false;
        let normalize_embedding = true;
        let (model, tokenizer) = build_model_and_tokenizer(&model_id, &revision, gpu, use_pth, approximate_gelu)?;
        let prompt = "The movie is awesome".to_string();
        let embedding = generate_embedding(tokenizer, model, prompt, normalize_embedding).await?;
        assert_eq!(*embedding.shape(), Shape::from(&[1, 6, 384]));
        Ok(())
    }

    #[test]
    fn test_build_model_and_tokenizer() -> anyhow::Result<()> {
        let model_id = DEFAULT_MODEL_ID.to_string();
        let revision = DEFAULT_REVISION.to_string();
        let gpu = false;
        let use_pth = false;
        let approximate_gelu = false;
        let result: Result<(BertModel, Tokenizer), anyhow::Error> = build_model_and_tokenizer(&model_id, &revision, gpu, use_pth, approximate_gelu);
        assert_eq!(result.is_ok(), true);
        Ok(())
    }

    #[test]
    fn test_normalize_l2() -> anyhow::Result<()> {
        let v = Tensor::new(&[[1.0, 2.0, 3.0]], &Device::Cpu)?;
        let v = normalize_l2(&v)?;
        // Expected result is [0, 0, 0] because the L2 norm of [1, 2, 3] is [0.2673, 0.5345, 0.8018]
        let result = v.eq(&Tensor::new(&[[0.2673, 0.5345, 0.8018]], &Device::Cpu)?);

        assert_eq!(result?.sum_all()?.to_scalar::<u8>()?, 0u8);
        Ok(())
    }

    #[tokio::test]
    async fn test_cosine_similarity_f32() -> anyhow::Result<()> {
        let a = Tensor::new(&[1.0f32, 2.0, 3.0], &Device::Cpu)?;
        let b = Tensor::new(&[1.0f32, 2.0, 3.0], &Device::Cpu)?;
        let similarity = cosine_similarity::<f32>(a, b).await?;
        assert_eq!(similarity, 1.0f32);
        Ok(())
    }

    #[tokio::test]
    async fn test_cosine_similarity_f64() -> anyhow::Result<()> {
        let a = Tensor::new(&[1.0f64, 2.0, 3.0], &Device::Cpu)?;
        let b = Tensor::new(&[1.0f64, 2.0, 3.0], &Device::Cpu)?;
        let similarity = cosine_similarity::<f64>(a, b).await?;
        assert_eq!(similarity, 1.0f64);
        Ok(())
    }

}