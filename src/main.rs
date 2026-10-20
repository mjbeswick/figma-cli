use std::{collections::VecDeque, fs, path::PathBuf};

use clap::{Parser, ValueEnum};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use url::Url;

#[derive(Parser, Debug)]
#[command(
    name = "figma",
    version,
    about = "Inspect Figma design files for agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Inspect a local Figma JSON export or a Figma file URL/key using the REST API
    Inspect {
        /// Local JSON file, Figma URL, or file key
        source: String,
        /// Include only matching node IDs (comma-separated or repeatable)
        #[arg(long = "node-id", value_delimiter = ',')]
        node_ids: Vec<String>,
        /// Include only nodes whose names contain this text (case-insensitive)
        #[arg(long)]
        query: Option<String>,
        /// Maximum child depth to include (root is depth 0)
        #[arg(long, default_value_t = 3)]
        depth: usize,
        /// Maximum number of node summaries to return
        #[arg(long, default_value_t = 500)]
        max_nodes: usize,
        /// Output representation
        #[arg(long, value_enum, default_value_t = InspectFormat::Json)]
        format: InspectFormat,
    },
    /// Print agent-oriented usage guidance for this CLI
    Skills {
        /// Output representation
        #[arg(long, value_enum, default_value_t = SkillsFormat::Markdown)]
        format: SkillsFormat,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum InspectFormat {
    Json,
    Text,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SkillsFormat {
    Json,
    Markdown,
}

#[derive(Debug, Error)]
enum AppError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("could not read local JSON file: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("FIGMA_ACCESS_TOKEN is required to inspect a Figma URL or key")]
    MissingToken,
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Figma API returned HTTP {status}: {message}")]
    Api { status: u16, message: String },
}

#[derive(Debug, Serialize)]
struct Inspection {
    file: FileSummary,
    filters: Filters,
    returned_nodes: usize,
    truncated: bool,
    nodes: Vec<NodeSummary>,
}

#[derive(Debug, Serialize)]
struct FileSummary {
    key: Option<String>,
    name: Option<String>,
    editor_type: Option<String>,
    version: Option<String>,
    last_modified: Option<String>,
    source: String,
}

#[derive(Debug, Serialize)]
struct Filters {
    node_ids: Vec<String>,
    query: Option<String>,
    depth: usize,
    max_nodes: usize,
}

#[derive(Debug, Serialize)]
struct NodeSummary {
    id: Option<String>,
    name: Option<String>,
    node_type: Option<String>,
    depth: usize,
    child_count: usize,
    properties: std::collections::BTreeMap<String, Value>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run(Cli::parse()).await {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), AppError> {
    match cli.command {
        Command::Skills { format } => match format {
            SkillsFormat::Markdown => print!("{AGENT_SKILL}"),
            SkillsFormat::Json => println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "name": "figma-design-inspection",
                    "description": "Inspect Figma file structure and design properties through the figma CLI.",
                    "instructions": AGENT_SKILL,
                    "commands": [
                        "figma inspect <FIGMA_URL|FILE_KEY|LOCAL_JSON>",
                        "figma inspect <source> --node-id <id> --depth <n>",
                        "figma inspect <source> --query <text> --max-nodes <n>",
                        "figma skills"
                    ]
                }))?
            ),
        },
        Command::Inspect {
            source,
            mut node_ids,
            query,
            depth,
            max_nodes,
            format,
        } => {
            if max_nodes == 0 {
                return Err(AppError::InvalidInput(
                    "--max-nodes must be greater than 0".into(),
                ));
            }
            if node_ids.is_empty() {
                node_ids = node_ids_from_url(&source);
            }
            node_ids = node_ids.into_iter().map(normalize_node_id).collect();
            let (data, key, source_label) = load_source(&source, &node_ids, depth).await?;
            let result = inspect(data, key, source_label, node_ids, query, depth, max_nodes);
            match format {
                InspectFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
                InspectFormat::Text => print_text(&result),
            }
        }
    }
    Ok(())
}

const AGENT_SKILL: &str = include_str!("../skills/figma-design-inspection/SKILL.md");

async fn load_source(
    source: &str,
    node_ids: &[String],
    depth: usize,
) -> Result<(Value, Option<String>, String), AppError> {
    let path = PathBuf::from(source);
    if path.is_file() {
        let data = serde_json::from_slice(&fs::read(&path)?)?;
        return Ok((data, None, path.display().to_string()));
    }

    let key = file_key(source)?;
    let token = std::env::var("FIGMA_ACCESS_TOKEN").map_err(|_| AppError::MissingToken)?;
    let base =
        std::env::var("FIGMA_API_BASE_URL").unwrap_or_else(|_| "https://api.figma.com".to_string());
    let mut url = Url::parse(&base)
        .map_err(|e| AppError::InvalidInput(format!("invalid FIGMA_API_BASE_URL: {e}")))?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("api.figma.com" | "api.figma-gov.com"))
        || url.username() != ""
        || url.password().is_some()
    {
        return Err(AppError::InvalidInput(
            "FIGMA_API_BASE_URL must use HTTPS on api.figma.com or api.figma-gov.com".into(),
        ));
    }
    {
        let mut segments = url.path_segments_mut().map_err(|_| {
            AppError::InvalidInput("FIGMA_API_BASE_URL cannot be a base URL".into())
        })?;
        segments.pop_if_empty();
        segments.extend(["v1", "files", &key]);
    }
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("depth", &(depth.saturating_add(1)).to_string());
        if !node_ids.is_empty() {
            query.append_pair("ids", &node_ids.join(","));
        }
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        "X-Figma-Token",
        HeaderValue::from_str(&token).map_err(|_| {
            AppError::InvalidInput("FIGMA_ACCESS_TOKEN contains invalid header characters".into())
        })?,
    );
    let response = reqwest::Client::builder()
        .default_headers(headers)
        .build()?
        .get(url)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        let message = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("err")
                    .or_else(|| v.get("message"))?
                    .as_str()
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| body.chars().take(500).collect());
        return Err(AppError::Api {
            status: status.as_u16(),
            message,
        });
    }
    let data = serde_json::from_str(&body)?;
    Ok((data, Some(key.clone()), format!("Figma REST API ({key})")))
}

fn file_key(source: &str) -> Result<String, AppError> {
    if let Ok(url) = Url::parse(source) {
        if url.scheme() != "https"
            || !matches!(
                url.host_str(),
                Some("figma.com" | "www.figma.com" | "api.figma.com")
            )
        {
            return Err(AppError::InvalidInput(
                "only HTTPS Figma URLs are accepted".into(),
            ));
        }
        let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
        let key = parts
            .windows(2)
            .find(|pair| {
                matches!(
                    pair[0],
                    "design" | "file" | "proto" | "board" | "slides" | "make"
                )
            })
            .map(|pair| pair[1])
            .filter(|key| !key.is_empty());
        return key.map(str::to_owned).ok_or_else(|| {
            AppError::InvalidInput(
                "could not find a file key after /design/, /file/, /proto/, /board/, /slides/, or /make/"
                    .into(),
            )
        });
    }
    if source.trim().is_empty()
        || source.contains('/')
        || source.contains('?')
        || source.contains('#')
    {
        return Err(AppError::InvalidInput(
            "expected a Figma file key, HTTPS Figma URL, or local JSON path".into(),
        ));
    }
    Ok(source.to_owned())
}

fn node_ids_from_url(source: &str) -> Vec<String> {
    Url::parse(source)
        .ok()
        .filter(|url| {
            url.scheme() == "https"
                && matches!(
                    url.host_str(),
                    Some("figma.com" | "www.figma.com" | "api.figma.com")
                )
        })
        .map(|url| {
            url.query_pairs()
                .filter(|(name, _)| name == "node-id")
                .map(|(_, id)| normalize_node_id(id.into_owned()))
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_node_id(id: String) -> String {
    if let Some((page, node)) = id.split_once('-')
        && page.bytes().all(|byte| byte.is_ascii_digit())
        && node.bytes().all(|byte| byte.is_ascii_digit())
        && !node.contains('-')
    {
        format!("{page}:{node}")
    } else {
        id
    }
}

fn inspect(
    data: Value,
    key: Option<String>,
    source: String,
    node_ids: Vec<String>,
    query: Option<String>,
    depth_limit: usize,
    max_nodes: usize,
) -> Inspection {
    let file = FileSummary {
        key,
        name: string_at(&data, "name"),
        editor_type: string_at(&data, "editorType"),
        version: string_at(&data, "version"),
        last_modified: string_at(&data, "lastModified"),
        source,
    };
    let root = data.get("document").unwrap_or(&data);
    let query_folded = query.as_deref().map(str::to_lowercase);
    let requested_ids: std::collections::HashSet<_> = node_ids.iter().map(String::as_str).collect();
    let mut queue = VecDeque::from([(root, 0usize, false)]);
    let mut nodes = Vec::new();
    let mut truncated = false;

    while let Some((node, node_depth, in_selected_subtree)) = queue.pop_front() {
        if node.is_object() {
            let id = string_at(node, "id");
            let name = string_at(node, "name");
            let selected_node = id.as_deref().is_some_and(|v| requested_ids.contains(v));
            let id_matches = requested_ids.is_empty() || in_selected_subtree || selected_node;
            let query_matches = query_folded.as_deref().is_none_or(|needle| {
                name.as_deref()
                    .is_some_and(|value| value.to_lowercase().contains(needle))
            });
            if id_matches && query_matches && nodes.len() < max_nodes {
                nodes.push(summarize_node(node, node_depth));
            }
            if node_depth < depth_limit {
                if let Some(children) = node.get("children").and_then(Value::as_array) {
                    queue.extend(children.iter().map(|child| {
                        (child, node_depth + 1, in_selected_subtree || selected_node)
                    }));
                }
            } else if node
                .get("children")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
            {
                truncated = true;
            }
        }
        if nodes.len() >= max_nodes && !queue.is_empty() {
            truncated = true;
            break;
        }
    }

    Inspection {
        file,
        filters: Filters {
            node_ids,
            query,
            depth: depth_limit,
            max_nodes,
        },
        returned_nodes: nodes.len(),
        truncated,
        nodes,
    }
}

fn summarize_node(node: &Value, depth: usize) -> NodeSummary {
    const INSPECTABLE_PROPERTIES: &[&str] = &[
        "absoluteBoundingBox",
        "absoluteRenderBounds",
        "absoluteTransform",
        "relativeTransform",
        "constraints",
        "clipsContent",
        "visible",
        "opacity",
        "blendMode",
        "cornerRadius",
        "rectangleCornerRadii",
        "fills",
        "strokes",
        "strokeWeight",
        "strokeAlign",
        "effects",
        "characters",
        "style",
        "styleId",
        "characterStyleOverrides",
        "styleOverrideTable",
        "componentId",
        "componentProperties",
        "layoutMode",
        "layoutWrap",
        "primaryAxisSizingMode",
        "counterAxisSizingMode",
        "primaryAxisAlignItems",
        "counterAxisAlignItems",
        "itemSpacing",
        "counterAxisSpacing",
        "paddingLeft",
        "paddingRight",
        "paddingTop",
        "paddingBottom",
        "layoutSizingHorizontal",
        "layoutSizingVertical",
        "minWidth",
        "maxWidth",
        "minHeight",
        "maxHeight",
        "exportSettings",
    ];
    let properties = INSPECTABLE_PROPERTIES
        .iter()
        .filter_map(|key| {
            node.get(*key)
                .cloned()
                .map(|value| ((*key).to_string(), value))
        })
        .collect();
    NodeSummary {
        id: string_at(node, "id"),
        name: string_at(node, "name"),
        node_type: string_at(node, "type"),
        depth,
        child_count: node
            .get("children")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        properties,
    }
}

fn string_at(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn print_text(result: &Inspection) {
    println!(
        "{}",
        result
            .file
            .name
            .as_deref()
            .unwrap_or("(unnamed Figma file)")
    );
    println!("Source: {}", result.file.source);
    if let Some(version) = &result.file.version {
        println!("Version: {version}");
    }
    for node in &result.nodes {
        let indent = "  ".repeat(node.depth.min(32));
        println!(
            "{indent}{} [{}] {} ({} children)",
            node.name.as_deref().unwrap_or("(unnamed)"),
            node.node_type.as_deref().unwrap_or("unknown"),
            node.id.as_deref().unwrap_or("no-id"),
            node.child_count
        );
    }
    if result.truncated {
        println!("Results truncated by depth or --max-nodes.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_file_key_from_figma_urls() {
        assert_eq!(
            file_key("https://www.figma.com/design/abc123/My-file?node-id=1%3A2").unwrap(),
            "abc123"
        );
        assert_eq!(file_key("https://figma.com/file/xyz/Title").unwrap(), "xyz");
        assert_eq!(
            file_key("https://www.figma.com/proto/n8LBDzH7lOh4SGUQ1xAE1z/Nutrismart?node-id=0-1")
                .unwrap(),
            "n8LBDzH7lOh4SGUQ1xAE1z"
        );
        assert_eq!(
            node_ids_from_url(
                "https://www.figma.com/proto/n8LBDzH7lOh4SGUQ1xAE1z/Nutrismart?node-id=0-1"
            ),
            vec!["0:1"]
        );
        assert_eq!(normalize_node_id("12:34".into()), "12:34");
    }

    #[test]
    fn rejects_non_figma_urls() {
        assert!(file_key("https://example.com/design/key/file").is_err());
        assert!(file_key("http://www.figma.com/design/key/file").is_err());
    }

    #[test]
    fn rejects_untrusted_api_base_urls() {
        for base in [
            "http://api.figma.com",
            "https://example.com",
            "https://attacker.example@api.figma.com",
        ] {
            let url = Url::parse(base).unwrap();
            assert!(
                url.scheme() != "https"
                    || !matches!(url.host_str(), Some("api.figma.com" | "api.figma-gov.com"))
                    || url.username() != ""
                    || url.password().is_some()
            );
        }
    }

    #[test]
    fn summarizes_a_local_document_tree_with_filters() {
        let data: Value = serde_json::from_str(
            r#"{"name":"Demo","version":"v1","document":{"id":"0:0","name":"Document","type":"DOCUMENT","children":[{"id":"1:1","name":"Home","type":"CANVAS","children":[{"id":"2:2","name":"Button","type":"RECTANGLE","fills":[{"type":"SOLID"}]}]}]}}"#,
        ).unwrap();
        let result = inspect(
            data,
            None,
            "fixture.json".into(),
            vec![],
            Some("button".into()),
            3,
            10,
        );
        assert_eq!(result.file.name.as_deref(), Some("Demo"));
        assert_eq!(result.returned_nodes, 1);
        assert_eq!(result.nodes[0].name.as_deref(), Some("Button"));
        assert_eq!(result.nodes[0].depth, 2);
        assert_eq!(
            result.nodes[0].properties["fills"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn local_node_id_filter_is_exact() {
        let data: Value = serde_json::from_str(r#"{"document":{"id":"0:0","type":"DOCUMENT","children":[{"id":"1:1","name":"One","type":"CANVAS"},{"id":"1:2","name":"Two","type":"CANVAS","children":[{"id":"2:1","name":"Child","type":"FRAME"}]},{"id":"1:3","name":"Three","type":"CANVAS"}]}}"#).unwrap();
        let result = inspect(
            data,
            None,
            "fixture.json".into(),
            vec!["1:2".into()],
            None,
            3,
            10,
        );
        assert_eq!(result.returned_nodes, 2);
        assert_eq!(result.nodes[0].id.as_deref(), Some("1:2"));
        assert_eq!(result.nodes[1].id.as_deref(), Some("2:1"));
    }
}
