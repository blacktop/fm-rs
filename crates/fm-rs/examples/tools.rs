//! Example: Tool calling with `FoundationModels`
//!
//! This example demonstrates how to define and use tools that the model
//! can invoke during generation to access external functionality.
//!
//! To run this example:
//!   `cargo run --example tools`

use fm_rs::{Error, GenerationOptions, Result, Session, SystemLanguageModel, Tool, ToolOutput};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct OpenMeteoResponse {
    current: OpenMeteoCurrent,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoCurrent {
    time: String,
    #[serde(rename = "temperature_2m")]
    temperature: f64,
    #[serde(rename = "relative_humidity_2m")]
    humidity: i64,
    #[serde(rename = "surface_pressure")]
    pressure: f64,
    #[serde(rename = "wind_speed_10m")]
    wind_speed: f64,
    #[serde(rename = "wind_direction_10m")]
    wind_direction: i64,
    #[serde(rename = "weather_code")]
    weather_code: i64,
}

#[derive(Debug, Deserialize)]
struct GeocodeResult {
    #[serde(rename = "display_name")]
    display_name: String,
    lat: String,
    lon: String,
    name: Option<String>,
}

struct Location {
    name: String,
    lat: f64,
    lon: f64,
}

/// A weather tool that fetches current weather data from Open-Meteo.
struct WeatherTool;

impl Tool for WeatherTool {
    fn name(&self) -> &'static str {
        "checkWeather"
    }

    fn description(&self) -> &'static str {
        "Check current weather conditions"
    }

    fn arguments_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "The city and country, e.g., 'Paris, France'"
                }
            },
            "required": ["location"]
        })
    }

    fn call(&self, arguments: Value) -> Result<ToolOutput> {
        let location = arguments
            .get("location")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Error::InvalidInput("Missing required argument: location".to_string())
            })?;

        let client = build_client()?;
        let geo = geocode_location(&client, location)?;
        let weather = fetch_open_meteo_weather(&client, geo.lat, geo.lon)?;

        let temp_f = weather.current.temperature * 9.0 / 5.0 + 32.0;
        let condition = get_weather_condition(weather.current.weather_code);
        let wind_dir = get_wind_direction(weather.current.wind_direction);
        let wind_mph = weather.current.wind_speed * 0.621_371;

        let weather_info = format!(
            "Current conditions for {}:\nTemperature: {:.1}°F ({:.1}°C)\nCondition: {}\nHumidity: {}%\nWind: {:.1} mph {}\nPressure: {:.1} hPa\nLast updated: {}",
            geo.name,
            temp_f,
            weather.current.temperature,
            condition,
            weather.current.humidity,
            wind_mph,
            wind_dir,
            weather.current.pressure,
            weather.current.time
        );

        Ok(ToolOutput::new(weather_info))
    }
}

fn build_client() -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("fm-rs/examples/tools")
        .build()
        .map_err(|err| Error::InternalError(format!("Failed to build HTTP client: {err}")))
}

fn geocode_location(client: &Client, location: &str) -> Result<Location> {
    let response: Vec<GeocodeResult> = client
        .get("https://nominatim.openstreetmap.org/search")
        .query(&[("q", location), ("format", "json"), ("limit", "1")])
        .send()
        .map_err(|err| Error::InternalError(format!("Failed to geocode location: {err}")))?
        .error_for_status()
        .map_err(|err| Error::InternalError(format!("Geocoding API request failed: {err}")))?
        .json()
        .map_err(|err| {
            Error::InternalError(format!("Failed to parse geocoding response: {err}"))
        })?;

    let first = response
        .into_iter()
        .next()
        .ok_or_else(|| Error::InvalidInput(format!("Location not found: {location}")))?;

    let lat = first
        .lat
        .parse::<f64>()
        .map_err(|err| Error::InternalError(format!("Invalid latitude: {err}")))?;
    let lon = first
        .lon
        .parse::<f64>()
        .map_err(|err| Error::InternalError(format!("Invalid longitude: {err}")))?;

    let name = first.name.unwrap_or(first.display_name);

    Ok(Location { name, lat, lon })
}

fn fetch_open_meteo_weather(client: &Client, lat: f64, lon: f64) -> Result<OpenMeteoResponse> {
    let query = [
        ("latitude", format!("{lat:.6}")),
        ("longitude", format!("{lon:.6}")),
        (
            "current",
            "temperature_2m,relative_humidity_2m,surface_pressure,wind_speed_10m,wind_direction_10m,weather_code"
                .to_string(),
        ),
        ("timezone", "auto".to_string()),
    ];

    client
        .get("https://api.open-meteo.com/v1/forecast")
        .query(&query)
        .send()
        .map_err(|err| Error::InternalError(format!("Failed to fetch weather data: {err}")))?
        .error_for_status()
        .map_err(|err| Error::InternalError(format!("Weather API request failed: {err}")))?
        .json::<OpenMeteoResponse>()
        .map_err(|err| Error::InternalError(format!("Failed to parse weather response: {err}")))
}

fn get_weather_condition(code: i64) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Foggy",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 | 65 => "Rain",
        66 | 67 => "Freezing rain",
        71 | 73 | 75 => "Snow",
        77 => "Snow grains",
        80..=82 => "Rain showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown",
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn get_wind_direction(degrees: i64) -> &'static str {
    const DIRECTIONS: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    let index = (((degrees as f64) + 11.25) / 22.5).floor() as usize;
    DIRECTIONS[index % DIRECTIONS.len()]
}

/// A calculator tool for basic math operations.
struct CalculatorTool;

impl Tool for CalculatorTool {
    fn name(&self) -> &'static str {
        "calculator"
    }

    fn description(&self) -> &'static str {
        "Performs basic arithmetic calculations. Supports add, subtract, multiply, divide."
    }

    fn arguments_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["add", "subtract", "multiply", "divide"],
                    "description": "The arithmetic operation to perform"
                },
                "a": {
                    "type": "number",
                    "description": "The first operand"
                },
                "b": {
                    "type": "number",
                    "description": "The second operand"
                }
            },
            "required": ["operation", "a", "b"]
        })
    }

    fn call(&self, arguments: Value) -> Result<ToolOutput> {
        let operation = arguments["operation"].as_str().unwrap_or("add");
        let a = arguments["a"].as_f64().unwrap_or(0.0);
        let b = arguments["b"].as_f64().unwrap_or(0.0);

        let result = match operation {
            "add" => a + b,
            "subtract" => a - b,
            "multiply" => a * b,
            "divide" => {
                if b == 0.0 {
                    return Ok(ToolOutput::new("Error: Division by zero"));
                }
                a / b
            }
            _ => return Ok(ToolOutput::new(format!("Unknown operation: {operation}"))),
        };

        Ok(ToolOutput::new(format!("{result}")))
    }
}

fn main() -> Result<()> {
    println!("FoundationModels Tool Calling Example");
    println!("=====================================\n");

    // Create the default system language model
    let model = SystemLanguageModel::new()?;

    if !model.is_available() {
        println!("FoundationModels is not available on this device.");
        return Ok(());
    }

    // Create tools
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(WeatherTool), Arc::new(CalculatorTool)];

    // Create a session with tools and explicit guidance to encourage tool usage
    let session = Session::with_instructions_and_tools(
        &model,
        "You are a tool-using assistant. When asked about weather, ALWAYS call the 'checkWeather' tool with the user's location. When asked to do any math or unit conversion, ALWAYS call the calculator tool. If the user provides a temperature value, treat it as math (no weather lookup) and use the calculator. Never refuse a conversion request.",
        &tools,
    )?;

    let options = GenerationOptions::builder()
        .temperature(0.5)
        .max_response_tokens(200)
        .build();

    // Example 1: Weather query (should trigger checkWeather tool)
    println!("--- Example 1: Weather Query ---");
    let prompt = "What's the weather like in Tokyo, Japan?";
    println!("Prompt: {prompt}");

    let response = session.respond(prompt, &options)?;
    println!("Response: {}\n", response.content());

    // Example 2: Math calculation (should trigger calculator tool)
    println!("--- Example 2: Math Calculation ---");
    let prompt = "What is 42 multiplied by 17?";
    println!("Prompt: {prompt}");

    let response = session.respond(prompt, &options)?;
    println!("Response: {}\n", response.content());

    // Example 3: Combined query
    println!("--- Example 3: Combined Query ---");
    let prompt = "If the temperature in Paris is 20 Celsius, what is that in Fahrenheit? Use the calculator tool and the formula: F = C * 9/5 + 32";
    println!("Prompt: {prompt}");

    let response = session.respond(prompt, &options)?;
    println!("Response: {}", response.content());

    println!("\n=====================================");
    println!("Tool calling example completed!");

    Ok(())
}
