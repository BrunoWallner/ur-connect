use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use reqwest::{
    Client, StatusCode, Url,
    cookie::Jar,
    header::{
        self, ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, HeaderMap, HeaderValue, ORIGIN, PRAGMA,
        REFERER, USER_AGENT,
    },
};

use crate::{
    model::TimetableEntry,
    parsing::{
        dom::{
            extract_flow_key_from_html, find_credential_fields, find_ics_url, find_input_value,
            find_timetable_menu_link, parse_document,
        },
        ics::parse_ics,
    },
};

pub struct UrConnect {
    client: Client,
    jar: Arc<Jar>,
    base_uri: Url,
    start_page: Url,
    login_post: Url,
    timetable_base: Url,
    flow_id: String,
}

struct FetchResult {
    body: String,
    final_url: Url,
    status: StatusCode,
}

impl UrConnect {
    pub fn new() -> Result<Self> {
        let base_uri = Url::parse("https://campusportal.ur.de")?;
        let start_page = base_uri.join("/qisserver/pages/cs/sys/portal/hisinoneStartPage.faces")?;
        let login_post = base_uri.join("/qisserver/rds?state=user&type=1&category=auth.login")?;
        let timetable_base = base_uri.join("/qisserver/pages/plan/individualTimetable.xhtml")?;
        let jar = Arc::new(Jar::default());

        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.5"));
        headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:128.0) Gecko/20100101 Firefox/128.0",
            ),
        );

        let client = Client::builder()
            .default_headers(headers)
            .cookie_provider(jar.clone())
            .timeout(Duration::from_secs(60))
            .build()?;

        Ok(Self {
            client,
            jar,
            base_uri,
            start_page,
            login_post,
            timetable_base,
            flow_id: "individualTimetableSchedule-flow".to_string(),
        })
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<()> {
        let start = self
            .get_with_headers(&self.start_page, Some(&self.start_page))
            .await
            .context("failed to load start page")?;

        let start_doc = parse_document(&start.body);
        let ajax_token = find_input_value(&start_doc, "input[name='ajax-token']", "value")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow::anyhow!("ajax-token not found on login form"))?;

        let (user_field, pass_field) = find_credential_fields(&start_doc);

        let cookie_domain = self.base_uri.domain().unwrap_or("");
        self.jar.add_cookie_str(
            &format!("_clickedButtonId=undefined; Domain={cookie_domain}; Path=/"),
            &self.base_uri,
        );

        let mut form = Vec::with_capacity(5);
        form.push(("userInfo".to_string(), String::new()));
        form.push(("ajax-token".to_string(), ajax_token));
        form.push((user_field, username.to_string()));
        form.push((pass_field, password.to_string()));
        form.push(("submit".to_string(), String::new()));

        let login_res = self
            .post_form_with_headers(&self.login_post, Some(&self.start_page), &form)
            .await
            .context("login request failed")?;

        if !login_res.status.is_success() {
            bail!("login failed with status {}", login_res.status);
        }

        let millis = Utc::now().timestamp_millis();
        self.jar.add_cookie_str(
            &format!("lastRefresh={millis}; Domain={cookie_domain}; Path=/"),
            &self.base_uri,
        );
        self.jar.add_cookie_str(
            &format!("sessionRefresh=0; Domain={cookie_domain}; Path=/"),
            &self.base_uri,
        );

        Ok(())
    }

    pub async fn get_timetable(&self) -> Result<Vec<TimetableEntry>> {
        let landing = self
            .get_with_headers(&self.start_page, Some(&self.start_page))
            .await
            .context("failed to load landing page after login")?;

        let entry_url = find_timetable_menu_link(&landing.body, &self.base_uri, &self.flow_id)
            .unwrap_or_else(|| build_timetable_uri(&self.timetable_base, &self.flow_id, None));

        let first = self
            .get_with_headers(&entry_url, Some(&self.start_page))
            .await
            .with_context(|| format!("failed to load timetable entry page at {entry_url}"))?;

        let flow_key = extract_flow_key_from_html(&first.body)
            .or_else(|| extract_flow_key_from_url(&first.final_url))
            .or_else(|| extract_flow_key_from_url(&entry_url))
            .ok_or_else(|| {
                anyhow::anyhow!("could not determine _flowExecutionKey for timetable")
            })?;

        let full_timetable_url =
            build_timetable_uri(&self.timetable_base, &self.flow_id, Some(&flow_key));

        let full_page = self
            .get_with_headers(&full_timetable_url, Some(&self.start_page))
            .await
            .with_context(|| {
                format!("failed to load full timetable page at {full_timetable_url}")
            })?;

        let ics_url = find_ics_url(&full_page.body, &self.base_uri)
            .or_else(|| find_ics_url(&first.body, &self.base_uri))
            .or_else(|| {
                let _ = std::fs::write("debug_timetable_full.html", &full_page.body);
                let _ = std::fs::write("debug_timetable_initial.html", &first.body);
                None
            })
            .ok_or_else(|| anyhow::anyhow!("could not locate ICS URL in timetable pages"))?;
        
        println!("ics URL: {}", &ics_url);

        let ics = self
            .get_with_headers(&ics_url, Some(&full_timetable_url))
            .await
            .with_context(|| format!("failed to download ICS from {ics_url}"))?;

        let entries = parse_ics(&ics.body);
        if entries.is_empty() {
            bail!("no events were parsed from the ICS response");
        }

        Ok(entries)
    }

    pub fn format_entries(entries: &[TimetableEntry]) -> String {
        if entries.is_empty() {
            return "No timetable entries found.".to_string();
        }
        entries
            .iter()
            .map(|entry| entry.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    async fn get_with_headers(&self, url: &Url, referer: Option<&Url>) -> Result<FetchResult> {
        let mut request = self.client.get(url.clone());
        if let Some(r) = referer {
            request = request.header(REFERER, r.as_str());
        }
        request = request
            .header("Upgrade-Insecure-Requests", "1")
            .header("Sec-Fetch-Dest", "document")
            .header("Sec-Fetch-Mode", "navigate")
            .header("Sec-Fetch-Site", "same-origin")
            .header(header::CONNECTION, "keep-alive");

        let response = request.send().await.context("HTTP GET request failed")?;
        let status = response.status();
        let final_url = response.url().clone();
        let body = response
            .text()
            .await
            .context("failed to read GET response body")?;

        Ok(FetchResult {
            body,
            final_url,
            status,
        })
    }

    async fn post_form_with_headers(
        &self,
        url: &Url,
        referer: Option<&Url>,
        form: &[(String, String)],
    ) -> Result<FetchResult> {
        let mut pairs: Vec<(&str, &str)> = Vec::with_capacity(form.len());
        for (k, v) in form {
            pairs.push((k.as_str(), v.as_str()));
        }

        let mut request = self.client.post(url.clone()).form(&pairs);
        if let Some(r) = referer {
            request = request.header(REFERER, r.as_str());
        }

        if let Some(authority) = self.base_uri.domain() {
            request = request.header(ORIGIN, format!("https://{authority}"));
        }

        request = request
            .header("Upgrade-Insecure-Requests", "1")
            .header("Sec-Fetch-Dest", "document")
            .header("Sec-Fetch-Mode", "navigate")
            .header("Sec-Fetch-Site", "same-origin")
            .header("Sec-Fetch-User", "?1")
            .header(PRAGMA, "no-cache")
            .header(CACHE_CONTROL, "no-cache");

        let response = request.send().await.context("HTTP POST request failed")?;
        let status = response.status();
        let final_url = response.url().clone();
        let body = response
            .text()
            .await
            .context("failed to read POST response body")?;

        Ok(FetchResult {
            body,
            final_url,
            status,
        })
    }
}

fn build_timetable_uri(base: &Url, flow_id: &str, flow_key: Option<&str>) -> Url {
    let mut result = base.clone();
    {
        let mut qp = result.query_pairs_mut();
        qp.clear();
        qp.append_pair("_flowId", flow_id);
        if let Some(key) = flow_key {
            qp.append_pair("_flowExecutionKey", key);
        }
    }
    result
}

fn extract_flow_key_from_url(url: &Url) -> Option<String> {
    for (key, value) in url.query_pairs() {
        if key == "_flowExecutionKey" {
            return Some(value.into_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::dom::contains_calendar_hint;

    #[test]
    fn formats_entries_into_lines() {
        let entries = vec![
            TimetableEntry::new(
                "2025-01-01".to_string(),
                "10:00 - 12:00".to_string(),
                "Sample Lecture".to_string(),
                "Room 101".to_string(),
                None,
            ),
            TimetableEntry::new(
                "2025-01-02".to_string(),
                "".to_string(),
                "Consultation".to_string(),
                "Building A".to_string(),
                None,
            ),
        ];

        let formatted = UrConnect::format_entries(&entries);
        assert!(formatted.contains("Sample Lecture @ Room 101"));
        assert!(formatted.contains("Consultation @ Building A"));
    }

    #[test]
    fn calendar_hint_matches_variants() {
        assert!(contains_calendar_hint("individualTimetableCalendarExport"));
        assert!(contains_calendar_hint("schedule.ics"));
    }

    #[test]
    fn parse_document_roundtrip() {
        let node = parse_document("<html><body><p>Hi</p></body></html>");
        let text = node.text_contents();
        assert!(text.contains("Hi"));
    }
}
