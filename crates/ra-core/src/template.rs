use serde::{Deserialize, Serialize};

/// Parameter definition for a workflow template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateParameter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
}

/// Info about an available template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateInfo {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub parameters: Vec<TemplateParameter>,
    #[serde(skip)]
    pub source: TemplateSource,
}

#[derive(Debug, Clone, Default)]
pub enum TemplateSource {
    File(String),
    #[default]
    Embedded,
}

/// A workflow template with its YAML content and metadata
#[derive(Debug, Clone)]
pub struct Template {
    pub info: TemplateInfo,
    pub yaml_content: String,
}

impl Template {
    /// Resolve template parameters in the YAML content
    pub fn resolve(
        &self,
        params: &std::collections::HashMap<String, String>,
    ) -> Result<String, String> {
        let mut content = self.yaml_content.clone();

        // Check required parameters
        for param in &self.info.parameters {
            let has_value = params.contains_key(&param.name);
            let has_default = param.default.is_some();

            if param.required && !has_value && !has_default {
                return Err(format!("Missing required parameter: {}", param.name));
            }
        }

        // Substitute all parameters
        for param in &self.info.parameters {
            let value = params
                .get(&param.name)
                .cloned()
                .or_else(|| param.default.clone())
                .unwrap_or_default();

            content = content.replace(&format!("{{{{{}}}}}", param.name), &value);
        }

        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_template() -> Template {
        Template {
            info: TemplateInfo {
                name: "test".to_string(),
                description: "test template".to_string(),
                parameters: vec![
                    TemplateParameter {
                        name: "path".to_string(),
                        description: "target path".to_string(),
                        required: true,
                        default: Some(".".to_string()),
                    },
                    TemplateParameter {
                        name: "model".to_string(),
                        description: "model to use".to_string(),
                        required: false,
                        default: Some("sonnet".to_string()),
                    },
                ],
                source: TemplateSource::Embedded,
            },
            yaml_content: "prompt: Analyze {{path}} with {{model}}".to_string(),
        }
    }

    #[test]
    fn test_resolve_with_params() {
        let t = make_template();
        let mut params = HashMap::new();
        params.insert("path".to_string(), "/src".to_string());
        params.insert("model".to_string(), "opus".to_string());

        let result = t.resolve(&params).unwrap();
        assert_eq!(result, "prompt: Analyze /src with opus");
    }

    #[test]
    fn test_resolve_with_defaults() {
        let t = make_template();
        let params = HashMap::new();

        let result = t.resolve(&params).unwrap();
        assert_eq!(result, "prompt: Analyze . with sonnet");
    }

    #[test]
    fn test_resolve_missing_required() {
        let t = Template {
            info: TemplateInfo {
                name: "test".to_string(),
                description: "test".to_string(),
                parameters: vec![TemplateParameter {
                    name: "target".to_string(),
                    description: "required param".to_string(),
                    required: true,
                    default: None,
                }],
                source: TemplateSource::Embedded,
            },
            yaml_content: "{{target}}".to_string(),
        };

        let params = HashMap::new();
        assert!(t.resolve(&params).is_err());
    }
}
