/// Module for relevance related functions

use std::collections::HashSet;
use once_cell::sync::Lazy;
use strsim::levenshtein;

use crate::common::NewsItem;

const MAX_DISTANCE: usize = 2;

static ROOT_WORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "Elección", "Política", "Reforma", "Proyecto", "Ley", "Congreso", "Senado", "Presidente",
        "Gobierno", "Primer", "Ministro", "Gabinete", "Oposición", "Coalición", "Democracia",
        "Constitución", "Parlamento", "Legislación", "Diplomático", "Tratado", "Sanción", "Embargo",
        "Resolución", "Comité", "Campaña", "Cabildeo", "Defensa", "Legislar", "Enmienda", "Presupuesto",
        "Déficit", "Superávit", "Economía", "Inflación", "Recesión", "Mercado", "Comercio", "Acciones",
        "Índice", "Moneda", "Inversión", "Fiscal", "Monetario", "Arancel", "Exportación", "Importación",
        "PIB", "Empleo", "Desempleo", "Pobreza", "Salud", "Vacuna", "Pandemia", "Brote", "Cuarentena",
        "Confinamiento", "Virus", "Infección", "Inmunidad", "Atención", "Hospital", "Clínica",
        "Tratamiento", "Diagnóstico", "Investigación", "Estudio", "Datos", "Estadísticas", "Encuesta",
        "Tecnología", "Innovación", "Software", "Hardware", "Red", "Internet", "Ciberseguridad",
        "Hackeo", "Brecha", "Cifrado", "IA", "Máquina", "Aprendizaje", "Robótica", "Automatización",
        "Cadena", "Criptomoneda", "Bitcoin", "Ethereum", "Social", "Medios", "Plataforma", "Aplicación",
        "Teléfono", "Móvil", "Satélite", "Espacio", "Exploración", "Clima", "Medio", "Emisión", "Carbono",
        "Verde", "Energía", "Renovable", "Solar", "Eólica", "Combustible", "Fósil", "Conservación",
        "Vida", "Biodiversidad", "Océano", "Plástico", "Contaminación", "Residuos", "Reciclar", "Guerra",
        "Conflicto", "Militar", "Seguridad", "Terrorismo", "Ataque", "Explosión", "Misil", "Nuclear",
        "Arma", "Dron", "Espía", "Inteligencia", "Refugiado", "Asilo", "Migración", "Frontera", "Visa",
        "Pasaporte", "Ciudadano", "Inmigración", "Deportación", "Humano", "Derechos", "Igualdad",
        "Justicia", "Corte", "Juez", "Juicio", "Jurado", "Veredicto", "Sentencia", "Apelación", "Crimen",
        "Robo", "Asesinato", "Fraude", "Soborno", "Corrupción", "Investigación", "Arresto", "Cargo",
        "Fianza", "Rescate", "Quiebra", "Ejecución", "Deuda", "Préstamo", "Interés", "Crédito", "Hipoteca",
        "Seguro", "Prima", "Reclamación", "Asegurado", "Litigio", "Patente", "Derecho", "Marca",
        "Infracción", "Acuerdo", "Fusión", "Adquisición", "Accionista", "Dividendo", "Capital", "Bono",
        "Rendimiento", "Cartera", "Activo", "Pasivo", "Auditoría", "Regulación", "Cumplimiento",
        "Estándar", "Procedimiento", "Protocolo", "Orientación", "Asesoramiento", "Consultoría",
        "Análisis", "Pronóstico", "Tendencia",
    ]
    .iter()
    .cloned()
    .collect()
});

// Function to check if any root word is within a certain Levenshtein distance
pub fn similar_to_root_word(word: &str, max_distance: usize) -> bool {
    let root_words = &*ROOT_WORDS;
    for root in root_words {
        if levenshtein(root, word) <= max_distance {
            return true;
        }
    }
    false
}

/// Function that calculates the relevance of a NewsItem
pub fn calculate_relevance(news_item: &NewsItem) -> u64 {
    
    // Calculate the relevance of the news item
    let mut relevance = 0;
    
    // If the new item has an error, return 0 relevance
    if news_item.error.is_some() {
        return relevance;
    }

    // If the news item has a creator not empty, increase the relevance by 1
    if news_item.creators.len() > 0 {
        relevance += 1;
    }

    // If any of the categories is similar to a root word, increase the relevance by 1 for each
    if let Some(categories) = &news_item.categories {
        for category in categories.split(",") {
            if similar_to_root_word(category, MAX_DISTANCE) {
                relevance += 1;
            }
        }
    }

    // If any of the keywords is similar to a root word, increase the relevance by 1 for each
    if let Some(keywords) = &news_item.keywords {
        for keyword in keywords.split(",") {
            if similar_to_root_word(keyword, MAX_DISTANCE) {
                relevance += 1;
            }
        }
    }

    // f any of the word in the title is similar to a root word, increase the relevance by 1 for each
    if news_item.title.len() > 0 {
        for word in news_item.title.split_whitespace() {
            if similar_to_root_word(word, MAX_DISTANCE) {
                relevance += 2;
            }
        }
    }

    // If any of the word in the description is similar to a root word, increase the relevance by 1 for each
    if news_item.description.len() > 0 {
        for word in news_item.description.split_whitespace() {
            if similar_to_root_word(word, MAX_DISTANCE) {
                relevance += 1;
            }
        }
    }

    // If any of the word in the clean content is similar to a root word, increase the relevance by 1 for each
    if let Some(clean_content) = &news_item.clean_content {
        for word in clean_content.split_whitespace() {
            if similar_to_root_word(word, MAX_DISTANCE) {
                relevance += 1;
            }
        }
    }

    relevance
}