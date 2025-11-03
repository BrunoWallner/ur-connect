use html_escape::decode_html_entities;
use kuchiki::{ElementData, NodeDataRef, NodeRef, traits::TendrilSink};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Url;

static ICS_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"https?://[A-Za-z0-9\-._~:/?#\[\]@!$&'()*+,;=%]+").unwrap());

pub fn parse_document(html: &str) -> NodeRef {
    kuchiki::parse_html().one(html)
}

pub fn select_elements(document: &NodeRef, selector: &str) -> Vec<NodeDataRef<ElementData>> {
    document
        .select(selector)
        .map(|iter| iter.collect())
        .unwrap_or_default()
}

pub fn find_input_value(document: &NodeRef, selector: &str, attr: &str) -> Option<String> {
    document
        .select(selector)
        .ok()?
        .next()
        .and_then(|node| node.attributes.borrow().get(attr).map(|v| v.to_string()))
}

pub fn find_credential_fields(document: &NodeRef) -> (String, String) {
    let mut user_field = None;
    let mut pass_field = None;

    if let Ok(inputs) = document.select("input") {
        for input in inputs {
            let attrs = input.attributes.borrow();
            let input_type = attrs
                .get("type")
                .map(|t| t.to_ascii_lowercase())
                .unwrap_or_default();

            if input_type == "password" && pass_field.is_none() {
                pass_field = attrs.get("name").map(|v| v.to_string());
            }

            if (input_type == "text" || input_type == "email") && user_field.is_none() {
                user_field = attrs.get("name").map(|v| v.to_string());
            }
        }
    }

    (
        user_field.unwrap_or_else(|| "asdf".to_string()),
        pass_field.unwrap_or_else(|| "fdsa".to_string()),
    )
}

pub fn find_timetable_menu_link(html: &str, base: &Url, flow_id: &str) -> Option<Url> {
    let document = parse_document(html);

    let flow_id_lower = flow_id.to_ascii_lowercase();
    let mut best: Option<(i32, Url)> = None;

    for node in select_elements(&document, "a[href]") {
        let href_value = {
            let attrs = node.attributes.borrow();
            attrs.get("href").map(|v| v.to_string())
        };
        let href = match href_value {
            Some(value) if !value.is_empty() => value,
            _ => continue,
        };

        let text = normalize_text(&text_content(&node));
        let href_lower = href.to_ascii_lowercase();
        let text_lower = text.to_ascii_lowercase();
        let has_flow_id = href_lower.contains(&format!("_flowid={}", flow_id_lower));
        let has_identifier = href_lower.contains("individualtimetable");
        let has_keyword = text_lower.contains("stundenplan") || text_lower.contains("timetable");

        let score = if has_flow_id {
            3
        } else if has_identifier {
            2
        } else if has_keyword {
            1
        } else {
            0
        };

        if score == 0 {
            continue;
        }

        let mut candidate_url = None;
        if let Ok(abs) = Url::parse(&href) {
            if abs.scheme().starts_with("http") {
                candidate_url = Some(abs);
            }
        }

        if candidate_url.is_none() {
            if let Ok(candidate) = base.join(&href) {
                if candidate.scheme().starts_with("http") {
                    candidate_url = Some(candidate);
                }
            }
        }

        if let Some(url) = candidate_url {
            match &mut best {
                Some((best_score, best_url)) => {
                    if score > *best_score {
                        *best_score = score;
                        *best_url = url;
                    }
                }
                None => {
                    best = Some((score, url));
                }
            }
        }
    }

    best.map(|(_, url)| url)
}

pub fn extract_flow_key_from_html(html: &str) -> Option<String> {
    let document = parse_document(html);

    for selector in ["input[name='_flowExecutionKey']", "input#_flowExecutionKey"] {
        if let Ok(mut matches) = document.select(selector) {
            if let Some(node) = matches.next() {
                if let Some(value) = node.attributes.borrow().get("value") {
                    let trimmed = value.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }

    if let Ok(mut matches) = document.select("a[href*='_flowExecutionKey=']") {
        if let Some(node) = matches.next() {
            if let Some(href) = node.attributes.borrow().get("href") {
                if let Some(key) = extract_flow_key_from_str(href) {
                    return Some(key);
                }
            }
        }
    }

    for meta in select_elements(&document, "meta[http-equiv]") {
        let attrs = meta.attributes.borrow();
        if let Some(http_equiv) = attrs.get("http-equiv") {
            if http_equiv.eq_ignore_ascii_case("refresh") {
                if let Some(content) = attrs.get("content") {
                    if let Some(idx) = content.to_ascii_lowercase().find("url=") {
                        let url_part = &content[idx + 4..];
                        if let Some(key) = extract_flow_key_from_str(url_part) {
                            return Some(key);
                        }
                    }
                }
            }
        }
    }

    static FLOW_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"_flowExecutionKey=([A-Za-z0-9]+)").unwrap());
    FLOW_REGEX
        .captures(html)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
}

pub fn find_ics_url(html: &str, base: &Url) -> Option<Url> {
    let document = parse_document(html);

    let textarea_selectors = [
        "textarea[id*='cal_add']",
        "textarea[id*='ical']",
        "textarea[id*='calendar']",
        "textarea[data-page-permalink]",
        "textarea[data-url]",
    ];

    for selector in &textarea_selectors {
        for node in select_elements(&document, selector) {
            if let Some(url) = try_node_for_calendar_url(&node, base) {
                return Some(url);
            }
        }
    }

    for node in select_elements(&document, "textarea") {
        if let Some(url) = try_node_for_calendar_url(&node, base) {
            return Some(url);
        }
    }

    for node in select_elements(&document, "input") {
        if let Some(url) = try_node_for_calendar_url(&node, base) {
            return Some(url);
        }
    }

    for node in select_elements(&document, "a[href]") {
        let mut values = attribute_values(&node, &["href"]);
        let text = normalize_text(&text_content(&node));
        if !text.is_empty() {
            values.push(text);
        }
        if let Some(url) = find_calendar_url_in_values(values, base) {
            return Some(url);
        }
    }

    for caps in ICS_REGEX.captures_iter(html) {
        if let Some(m) = caps.get(0) {
            let candidate = decode_html_entities(m.as_str()).trim().to_string();
            if contains_calendar_hint(&candidate) || candidate.to_ascii_lowercase().contains(".ics")
            {
                if let Some(url) = resolve_url(&candidate, base) {
                    return Some(url);
                }
            }
        }
    }

    None
}

pub fn extract_flow_key_from_str(input: &str) -> Option<String> {
    if let Ok(url) = Url::parse(input) {
        for (key, value) in url.query_pairs() {
            if key == "_flowExecutionKey" {
                return Some(value.into_owned());
            }
        }
    }

    if let Some(idx) = input.find("_flowExecutionKey=") {
        let rest = &input[idx + "_flowExecutionKey=".len()..];
        let key: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if !key.is_empty() {
            return Some(key);
        }
    }
    None
}

pub fn text_content(node: &NodeDataRef<ElementData>) -> String {
    node.text_contents()
}

pub fn normalize_text(input: &str) -> String {
    decode_html_entities(input)
        .replace('\u{00A0}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn contains_calendar_hint(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("calendarexport")
        || lower.contains("calendar")
        || lower.contains("individualtimetablecalendarexport")
        || lower.contains("timetablecalendar")
        || lower.contains(".ics")
        || lower.contains("ical")
}

fn try_node_for_calendar_url(node: &NodeDataRef<ElementData>, base: &Url) -> Option<Url> {
    let mut values = Vec::new();
    let text = normalize_text(&text_content(node));
    if !text.is_empty() {
        values.push(text);
    }
    values.extend(attribute_values(
        node,
        &[
            "data-page-permalink",
            "data-page-permalink-title",
            "data-url",
            "value",
        ],
    ));
    find_calendar_url_in_values(values, base)
}

fn attribute_values(node: &NodeDataRef<ElementData>, keys: &[&str]) -> Vec<String> {
    let mut values = Vec::new();
    {
        let attrs = node.attributes.borrow();
        for key in keys {
            if let Some(value) = attrs.get(*key) {
                let decoded = decode_html_entities(value).trim().to_string();
                if !decoded.is_empty() {
                    values.push(decoded);
                }
            }
        }
    }
    values
}

fn find_calendar_url_in_values(values: Vec<String>, base: &Url) -> Option<Url> {
    for value in values {
        if value.is_empty() {
            continue;
        }
        if !contains_calendar_hint(&value) {
            continue;
        }
        if let Some(url) = resolve_url(&value, base) {
            return Some(url);
        }
    }
    None
}

fn resolve_url(candidate: &str, base: &Url) -> Option<Url> {
    if candidate.is_empty() {
        return None;
    }
    if let Ok(abs) = Url::parse(candidate) {
        if abs.scheme().starts_with("http") {
            return Some(abs);
        }
    }
    if let Ok(joined) = base.join(candidate) {
        if joined.scheme().starts_with("http") {
            return Some(joined);
        }
    }
    None
}