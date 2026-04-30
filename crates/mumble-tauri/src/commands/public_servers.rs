//! Public Mumble server directory + file-server capability probe.

/// A public Mumble server from the official directory.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub(crate) struct PublicServer {
    name: String,
    country: String,
    country_code: String,
    ip: String,
    port: u16,
    region: String,
    url: String,
}

/// XML wrapper: `<servers><server .../> ...</servers>`
#[derive(serde::Deserialize, Debug)]
struct ServersXml {
    #[serde(rename = "server", default)]
    server: Vec<ServerXml>,
}

/// A single `<server ... />` element with attributes.
#[derive(serde::Deserialize, Debug)]
struct ServerXml {
    #[serde(rename = "@name", default)]
    name: String,
    #[serde(rename = "@country", default)]
    country: String,
    #[serde(rename = "@country_code", default)]
    country_code: String,
    #[serde(rename = "@ip", default)]
    ip: String,
    #[serde(rename = "@port", default = "default_port")]
    port: u16,
    #[serde(rename = "@region", default)]
    region: String,
    #[serde(rename = "@url", default)]
    url: String,
}

fn default_port() -> u16 {
    64738
}

/// Parse the Mumble public server list XML into a vec of [`PublicServer`].
fn parse_public_server_xml(xml: &str) -> Result<Vec<PublicServer>, String> {
    let parsed: ServersXml =
        quick_xml::de::from_str(xml).map_err(|e| format!("XML parse error: {e}"))?;

    Ok(parsed
        .server
        .into_iter()
        .map(|s| PublicServer {
            name: s.name,
            country: s.country,
            country_code: s.country_code,
            ip: s.ip,
            port: s.port,
            region: s.region,
            url: s.url,
        })
        .collect())
}

/// Fetch the official Mumble public server list.
///
/// The list is served as XML from `https://publist.mumble.info/v1/list`.
#[tauri::command]
pub(crate) async fn fetch_public_servers() -> Result<Vec<PublicServer>, String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) FancyMumble/1.0 Safari/537.36")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let response = client
        .get("https://publist.mumble.info/v1/list")
        .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch public server list: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("Server returned HTTP {status}"));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    tracing::debug!("Public server list: {} bytes received", body.len());

    let servers = parse_public_server_xml(&body)?;

    tracing::debug!("Fetched {} public servers", servers.len());
    Ok(servers)
}

/// Probe the file-server plugin's `GET /capabilities` endpoint.
///
/// Performed in Rust (rather than via browser `fetch`) so it works
/// regardless of the file-server's CORS allow-list configuration and
/// avoids preflight overhead.  Returns the raw JSON body on success;
/// the frontend deserialises into [`FileServerCapabilities`].
#[tauri::command]
pub(crate) async fn fetch_file_server_capabilities(base_url: String) -> Result<String, String> {
    let trimmed = base_url.trim_end_matches('/');
    let url = format!("{trimmed}/capabilities");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| format!("HTTP client init failed: {e}"))?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    response
        .text()
        .await
        .map_err(|e| format!("read body failed: {e}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn parse_single_server() {
        let xml = r#"<servers><server name="Test Server" ca="1" continent_code="EU" country="Germany" country_code="DE" ip="mumble.example.com" port="64738" region="Bavaria" url="https://example.com"/></servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(
            servers[0],
            PublicServer {
                name: "Test Server".into(),
                country: "Germany".into(),
                country_code: "DE".into(),
                ip: "mumble.example.com".into(),
                port: 64738,
                region: "Bavaria".into(),
                url: "https://example.com".into(),
            }
        );
    }

    #[test]
    fn parse_multiple_servers() {
        let xml = r#"<servers>
            <server name="Alpha" ca="0" continent_code="NA" country="Canada" country_code="CA" ip="1.2.3.4" port="12345" region="Ontario" url="https://alpha.ca"/>
            <server name="Beta" ca="1" continent_code="AS" country="Japan" country_code="JP" ip="5.6.7.8" port="64738" region="Tokyo" url="https://beta.jp"/>
            <server name="Gamma" ca="0" continent_code="EU" country="France" country_code="FR" ip="fr.example.com" port="9999" region="Paris" url=""/>
        </servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers.len(), 3);
        assert_eq!(servers[0].name, "Alpha");
        assert_eq!(servers[0].country_code, "CA");
        assert_eq!(servers[0].port, 12345);
        assert_eq!(servers[1].name, "Beta");
        assert_eq!(servers[1].country, "Japan");
        assert_eq!(servers[2].name, "Gamma");
        assert_eq!(servers[2].ip, "fr.example.com");
        assert_eq!(servers[2].port, 9999);
    }

    #[test]
    fn parse_empty_server_list() {
        let xml = r#"<servers></servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert!(servers.is_empty());
    }

    #[test]
    fn parse_self_closing_servers_tag() {
        let xml = r#"<servers/>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert!(servers.is_empty());
    }

    #[test]
    fn parse_default_port_when_missing() {
        let xml = r#"<servers><server name="NoPort" country="US" country_code="US" ip="10.0.0.1" region="Test" url=""/></servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].port, 64738);
    }

    #[test]
    fn parse_special_characters_in_name() {
        let xml = r#"<servers><server name="&lt;Cool&amp;Server&gt;" country="US" country_code="US" ip="10.0.0.1" port="64738" region="Test" url=""/></servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers[0].name, "<Cool&Server>");
    }

    #[test]
    fn parse_unicode_in_name() {
        let xml = r#"<servers><server name="Mumble Deutsch" country="Germany" country_code="DE" ip="10.0.0.1" port="64738" region="NRW" url=""/></servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers[0].name, "Mumble Deutsch");
        assert_eq!(servers[0].country, "Germany");
    }

    #[test]
    fn parse_invalid_xml_returns_error() {
        let xml = r#"<servers><server name="broken"</servers>"#;
        let result = parse_public_server_xml(xml);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("XML parse error"));
    }

    #[test]
    fn parse_extra_attributes_are_ignored() {
        let xml = r#"<servers><server name="Extra" ca="1" continent_code="EU" country="UK" country_code="GB" ip="10.0.0.1" port="64738" region="London" url="https://uk.example.com" extra_field="ignored"/></servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "Extra");
    }

    #[test]
    fn parse_realistic_snippet() {
        let xml = r#"<servers>
<server name="`JOIN RADIO BRIKER NUSANTARA`" ca="0" continent_code="AS" country="Singapore" country_code="SG" ip="beve-studio.my.id" port="10622" region="Singapore" url="https://www.mumble.info/"/>
<server name="Comms" ca="1" continent_code="EU" country="Germany" country_code="DE" ip="mumble.natenom.dev" port="64738" region="Baden-Wurttemberg" url="https://natenom.dev"/>
</servers>"#;
        let servers = parse_public_server_xml(xml).unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "`JOIN RADIO BRIKER NUSANTARA`");
        assert_eq!(servers[0].country_code, "SG");
        assert_eq!(servers[0].port, 10622);
        assert_eq!(servers[1].ip, "mumble.natenom.dev");
        assert_eq!(servers[1].country, "Germany");
    }
}
