use crate::im::core::{i18n::ImText, thread::ThreadCreateDefaults};
use crate::im_runtime::ThreadCreateDraftState;

pub(crate) const FEISHU_PROJECT_PAGE_SIZE: usize = 20;

use super::super::common::build_markdown_card;
use super::super::markdown::normalize_card_markdown;
use super::common::build_interactive_choice_block;
pub fn build_thread_create_settings_card(
    request_id: &str,
    defaults: &ThreadCreateDefaults,
    text: ImText,
) -> serde_json::Value {
    build_thread_create_settings_page_card(
        request_id,
        defaults,
        &ThreadCreateDraftState::default(),
        1,
        text,
    )
}

pub fn build_thread_create_settings_page_card(
    request_id: &str,
    defaults: &ThreadCreateDefaults,
    draft: &ThreadCreateDraftState,
    page: usize,
    text: ImText,
) -> serde_json::Value {
    let total_pages = defaults
        .projects
        .len()
        .div_ceil(FEISHU_PROJECT_PAGE_SIZE)
        .max(1);
    let page = page.clamp(1, total_pages);
    let cwd_options = thread_cwd_options(defaults, draft, page, text);
    let model_options = thread_model_options(defaults, text);
    let default_line = |label: &str, value: Option<String>| {
        text.field_line(
            label,
            &value
                .as_deref()
                .map(normalize_card_markdown)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| text.codex_app_default_value().to_string()),
        )
    };
    let remote_line = text.field_line(
        text.remote_label(),
        &defaults
            .remote_name
            .as_ref()
            .map(|value| normalize_card_markdown(value))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| text.not_connected().to_string()),
    );
    let mut settings_lines = vec![
        remote_line,
        default_line(text.cwd_label(), defaults.cwd.clone()),
        default_line(text.provider_label(), defaults.model_provider.clone()),
        default_line(text.model_label(), defaults.model.clone()),
    ];
    if defaults.capabilities.reasoning {
        settings_lines.push(default_line(text.effort_label(), defaults.effort.clone()));
    }
    if defaults.capabilities.permissions {
        settings_lines.push(default_line(
            text.permission_label_title(),
            defaults
                .permission
                .as_deref()
                .map(|permission| text.permission_label(permission)),
        ));
    }
    if let Some(notice) = defaults
        .settings_notice
        .as_deref()
        .filter(|notice| !notice.trim().is_empty())
    {
        settings_lines.push(normalize_card_markdown(notice));
    }
    let mut form_elements = vec![
        serde_json::json!({
            "tag": "markdown",
            "content": format!("**{}**", text.cwd_section())
        }),
        select_static_element("cwd_choice", text.cwd_select_placeholder(), cwd_options),
        serde_json::json!({
            "tag": "input",
            "name": "cwd_custom",
            "required": false,
            "placeholder": {
                "tag": "plain_text",
                "content": text.cwd_custom_placeholder()
            }
        }),
        serde_json::json!({
            "tag": "markdown",
            "content": format!("**{}**", text.model_section())
        }),
        select_static_element("model", text.model_select_placeholder(), model_options),
    ];
    if total_pages > 1 {
        let buttons = [("prev", "上一页项目", page > 1), ("next", "下一页项目", page < total_pages)]
            .into_iter()
            .filter(|(_, _, visible)| *visible)
            .map(|(direction, label, _)| serde_json::json!({
                "tag": "button", "type": "default",
                "text": {"tag":"plain_text", "content":label},
                "name": format!("cwd_page_{direction}"), "form_action_type":"submit",
                "behaviors":[{"type":"callback", "value":{
                    "kind":"thread_route_create_cwd_page", "requestId":request_id, "page":page, "direction":direction
                }}]
            }))
            .collect::<Vec<_>>();
        form_elements.insert(2, serde_json::json!({
            "tag":"markdown", "content":format!("项目目录第 {page}/{total_pages} 页；翻页保留已填写的设置。")
        }));
        form_elements.insert(
            3,
            serde_json::json!({
                "tag":"column_set", "flex_mode":"none", "columns":[{
                    "tag":"column", "width":"auto", "elements":buttons
                }]
            }),
        );
    }
    if defaults.capabilities.reasoning {
        form_elements.extend([
            serde_json::json!({
                "tag": "markdown",
                "content": format!("**{}**", text.effort_section())
            }),
            select_static_element(
                "effort",
                text.effort_select_placeholder(),
                thread_effort_options(defaults, text),
            ),
        ]);
    }
    if defaults.capabilities.permissions {
        form_elements.extend([
            serde_json::json!({
                "tag": "markdown",
                "content": format!("**{}**", text.permission_section())
            }),
            select_static_element(
                "permission",
                text.permission_select_placeholder(),
                thread_permission_options(text),
            ),
        ]);
    }
    // Page navigation submits the form too. Restore every field so choosing a
    // project on another page never resets a typed path or the selected model.
    for element in &mut form_elements {
        let value = match element.get("name").and_then(serde_json::Value::as_str) {
            Some("cwd_choice") => draft.cwd_choice.as_deref(),
            Some("cwd_custom") => draft.cwd_custom.as_deref(),
            Some("model") => draft.model.as_deref(),
            Some("effort") => draft.effort.as_deref(),
            Some("permission") => draft.permission.as_deref(),
            _ => None,
        };
        if let Some(value) = value {
            if element["tag"] == "input" {
                element["default_value"] = serde_json::json!(value);
            } else if let Some(index) = element["options"]
                .as_array()
                .and_then(|options| options.iter().position(|option| option["value"] == value))
            {
                element["initial_index"] = serde_json::json!(index + 1);
            }
        }
    }
    form_elements.push(serde_json::json!({
        "tag": "column_set",
        "flex_mode": "none",
        "columns": [{
            "tag": "column",
            "width": "auto",
            "elements": [{
                "tag": "button",
                "type": "primary",
                "text": {
                    "tag": "plain_text",
                    "content": text.confirm_create_button()
                },
                "name": "submit_thread_create",
                "form_action_type": "submit",
                "behaviors": [{
                    "type": "callback",
                    "value": {
                        "kind": "thread_route_create_submit",
                        "requestId": request_id
                    }
                }]
            }]
        }]
    }));
    let mut elements = vec![
        serde_json::json!({
            "tag": "markdown",
            "content": text.create_settings_card_intro()
        }),
        serde_json::json!({
            "tag": "markdown",
            "content": format!("<font color='grey'>{}</font>", settings_lines.join("\n"))
        }),
        serde_json::json!({
            "tag": "form",
            "name": "thread_create_form",
            "element_id": "thread_create_form",
            "direction": "vertical",
            "vertical_spacing": "8px",
            "elements": form_elements
        }),
    ];
    elements.push(build_interactive_choice_block(
        text.create_default_button(),
        text.create_default_description(),
        serde_json::json!({
            "kind": "thread_route_create_default",
            "requestId": request_id
        }),
        false,
        false,
        false,
        text,
    ));
    elements.push(build_interactive_choice_block(
        text.back_button(),
        text.back_description(),
        serde_json::json!({
            "kind": "thread_route_choice",
            "requestId": request_id,
            "action": "back"
        }),
        false,
        false,
        false,
        text,
    ));

    let mut card = build_markdown_card("", Some(text.create_settings_card_title()), Some("indigo"));
    card["body"]["padding"] = serde_json::json!("8px 8px 8px 8px");
    card["body"]["vertical_spacing"] = serde_json::json!("8px");
    card["body"]["elements"] = serde_json::Value::Array(elements);
    card
}

fn select_static_element(
    name: &str,
    placeholder: &str,
    options: Vec<(String, String)>,
) -> serde_json::Value {
    serde_json::json!({
        "tag": "select_static",
        "name": name,
        "required": false,
        "placeholder": {
            "tag": "plain_text",
            "content": placeholder
        },
        "options": options
            .into_iter()
            .map(|(label, value)| {
                serde_json::json!({
                    "text": {
                        "tag": "plain_text",
                        "content": label
                    },
                    "value": value
                })
            })
            .collect::<Vec<_>>()
    })
}

fn thread_cwd_options(
    defaults: &ThreadCreateDefaults,
    draft: &ThreadCreateDraftState,
    page: usize,
    text: ImText,
) -> Vec<(String, String)> {
    let mut options = vec![(
        text.use_default_cwd().to_string(),
        "__default__".to_string(),
    )];
    for project in defaults
        .projects
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .skip((page - 1) * FEISHU_PROJECT_PAGE_SIZE)
        .take(FEISHU_PROJECT_PAGE_SIZE)
    {
        options.push((project_option_label_with_path(project), project.to_string()));
    }
    if let Some(selected) = draft
        .cwd_choice
        .as_deref()
        .filter(|value| !value.starts_with("__"))
        && !options.iter().any(|(_, value)| value == selected)
    {
        options.push((
            project_option_label_with_path(selected),
            selected.to_owned(),
        ));
    }
    options.push((
        text.custom_cwd_label().to_string(),
        "__custom__".to_string(),
    ));
    dedupe_options(options)
}

fn thread_model_options(defaults: &ThreadCreateDefaults, text: ImText) -> Vec<(String, String)> {
    let mut options = vec![(
        text.use_current_model().to_string(),
        "__default__".to_string(),
    )];
    for model in defaults
        .models
        .iter()
        .filter(|value| !value.value.trim().is_empty())
    {
        options.push((model.label.clone(), model.value.clone()));
    }
    dedupe_options(options)
}

fn thread_effort_options(defaults: &ThreadCreateDefaults, text: ImText) -> Vec<(String, String)> {
    let mut options = vec![(
        text.use_model_default_effort().to_string(),
        "__default__".to_string(),
    )];
    if let Some(effort) = defaults.effort.as_deref().map(str::trim)
        && !effort.is_empty()
    {
        options.push((text.reasoning_effort_label(effort), effort.to_string()));
    }
    for effort in defaults
        .efforts
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        options.push((text.reasoning_effort_label(effort), effort.to_string()));
    }
    dedupe_options(options)
}

fn thread_permission_options(text: ImText) -> Vec<(String, String)> {
    vec![
        (
            text.default_permission_label().to_string(),
            "workspace_user".to_string(),
        ),
        (
            text.auto_review_label().to_string(),
            "auto_review".to_string(),
        ),
        (
            text.full_access_label().to_string(),
            "full_access".to_string(),
        ),
    ]
}

fn dedupe_options(options: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut output = Vec::new();
    for (label, value) in options {
        if !output
            .iter()
            .any(|(_, existing_value): &(String, String)| existing_value == &value)
        {
            output.push((label, value));
        }
    }
    output
}

fn project_option_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string())
}

fn project_option_label_with_path(path: &str) -> String {
    let name = project_option_label(path);
    if name == path {
        path.to_string()
    } else {
        format!("{name} - {path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::im::core::thread::ThreadCreateCapabilities;

    #[test]
    fn project_pages_keep_all_candidates_and_form_values() {
        let defaults = ThreadCreateDefaults {
            projects: (0..45).map(|index| format!("D:/fixture/{index}")).collect(),
            models: vec![crate::im::core::thread::ThreadModelChoice {
                label: "fixture model".into(),
                value: "fixture-model".into(),
            }],
            ..Default::default()
        };
        let draft = ThreadCreateDraftState {
            cwd_choice: Some("D:/fixture/3".into()),
            cwd_custom: Some("D:/typed path".into()),
            model: Some("fixture-model".into()),
            ..Default::default()
        };
        let all: std::collections::HashSet<_> = (1..=3)
            .flat_map(|page| thread_cwd_options(&defaults, &draft, page, ImText::zh_cn()))
            .map(|(_, value)| value)
            .collect();
        for path in &defaults.projects {
            assert!(all.contains(path));
        }
        let card = build_thread_create_settings_page_card(
            "fixture-pages",
            &defaults,
            &draft,
            2,
            ImText::zh_cn(),
        );
        let elements = card
            .pointer("/body/elements/2/elements")
            .unwrap()
            .as_array()
            .unwrap();
        let field = |name: &str| elements.iter().find(|item| item["name"] == name).unwrap();
        let selected_value = |name: &str| {
            let element = field(name);
            let index = element["initial_index"].as_u64().unwrap() as usize - 1;
            element["options"][index]["value"].clone()
        };
        assert_eq!(selected_value("cwd_choice"), "D:/fixture/3");
        assert_eq!(field("cwd_custom")["default_value"], "D:/typed path");
        assert_eq!(selected_value("model"), "fixture-model");
        let serialized = card.to_string();
        assert!(serialized.contains("thread_route_create_cwd_page"));
        assert!(serialized.contains("上一页项目") && serialized.contains("下一页项目"));
        assert!(serialized.contains("第 2/3 页"));
    }

    #[test]
    fn form_fields_follow_executor_capabilities_and_preserve_common_callbacks() {
        for supported in [true, false] {
            let defaults = ThreadCreateDefaults {
                capabilities: ThreadCreateCapabilities {
                    reasoning: supported,
                    permissions: supported,
                },
                settings_notice: (!supported).then(|| "模型与工具设置由当前执行端继承".to_string()),
                ..Default::default()
            };
            let card = build_thread_create_settings_card("request-1", &defaults, ImText::zh_cn());
            let elements = card
                .pointer("/body/elements/2/elements")
                .unwrap()
                .as_array()
                .unwrap();
            let has_field = |name: &str| elements.iter().any(|element| element["name"] == name);
            assert!(has_field("cwd_choice"));
            assert!(has_field("cwd_custom"));
            assert!(has_field("model"));
            assert_eq!(has_field("effort"), supported);
            assert_eq!(has_field("permission"), supported);
            let submit = elements
                .last()
                .unwrap()
                .pointer("/columns/0/elements/0/behaviors/0/value")
                .unwrap();
            assert_eq!(submit["kind"], "thread_route_create_submit");
            assert_eq!(submit["requestId"], "request-1");
            if let Some(notice) = defaults.settings_notice {
                assert!(
                    card.pointer("/body/elements/1/content")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .contains(&notice)
                );
            }
        }
    }
}
