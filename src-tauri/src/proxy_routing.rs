use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RoutingMode {
    Proxy,
    Bypass,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationRouting {
    pub mode: RoutingMode,
    pub applications: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DomainRouting {
    pub mode: RoutingMode,
    pub domains: Vec<String>,
}

impl ApplicationRouting {
    pub fn normalize(&mut self) -> Result<(), String> {
        let mut applications = Vec::new();
        for application in &self.applications {
            let application = application.trim();
            if application.is_empty() || application.contains('\0') {
                return Err(
                    "Application identifiers must not be empty or contain NUL characters".into(),
                );
            }
            if !applications.iter().any(|existing| existing == application) {
                applications.push(application.to_owned());
            }
        }
        if applications.is_empty() {
            return Err("Application routing requires at least one application".into());
        }
        self.applications = applications;
        Ok(())
    }
}

impl DomainRouting {
    pub fn normalize(&mut self) -> Result<(), String> {
        let mut domains = Vec::new();
        for input in &self.domains {
            let input = input.trim().to_ascii_lowercase();
            let (prefix, hostname) = if let Some(hostname) = input.strip_prefix("full:") {
                ("full", hostname)
            } else {
                let hostname = input.strip_prefix("domain:").unwrap_or(&input);
                ("domain", hostname.strip_prefix("*.").unwrap_or(hostname))
            };
            let hostname = hostname.trim_end_matches('.');
            if hostname.is_empty()
                || hostname.len() > 253
                || hostname.split('.').any(|label| {
                    label.is_empty()
                        || label.len() > 63
                        || label.starts_with('-')
                        || label.ends_with('-')
                        || !label
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                })
            {
                return Err(format!("Invalid routing domain: {input}; use a hostname, domain:hostname or full:hostname"));
            }
            let domain = format!("{prefix}:{hostname}");
            if !domains.contains(&domain) {
                domains.push(domain);
            }
        }
        if domains.is_empty() {
            return Err("Domain routing requires at least one domain".into());
        }
        self.domains = domains;
        Ok(())
    }
}

enum PathPart {
    Field(String),
    Index(usize),
}

// 与 Xray --set 相同的 field[index].field 路径规则，按原始顺序还原配置。
fn parse_path(path: &str) -> Result<Vec<PathPart>, String> {
    let bytes = path.as_bytes();
    let mut parts = Vec::new();
    let mut position = 0;
    while position < bytes.len() {
        let start = position;
        while position < bytes.len() && !matches!(bytes[position], b'.' | b'[' | b']') {
            position += 1;
        }
        if start == position {
            return Err(format!("Invalid Xray --set path: {path}"));
        }
        parts.push(PathPart::Field(path[start..position].into()));
        while position < bytes.len() && bytes[position] == b'[' {
            position += 1;
            let start = position;
            while position < bytes.len() && bytes[position].is_ascii_digit() {
                position += 1;
            }
            if start == position || position == bytes.len() || bytes[position] != b']' {
                return Err(format!("Invalid Xray --set array index: {path}"));
            }
            parts.push(PathPart::Index(
                path[start..position]
                    .parse()
                    .map_err(|_| format!("Invalid Xray --set array index: {path}"))?,
            ));
            position += 1;
        }
        if position == bytes.len() {
            break;
        }
        if bytes[position] != b'.' || position + 1 == bytes.len() {
            return Err(format!("Invalid Xray --set path: {path}"));
        }
        position += 1;
    }
    if parts.is_empty() {
        return Err("Xray --set path cannot be empty".into());
    }
    Ok(parts)
}

fn assign(node: &mut Value, path: &[PathPart], value: Value) -> Result<(), String> {
    let Some((part, remaining)) = path.split_first() else {
        *node = value;
        return Ok(());
    };
    match part {
        PathPart::Field(field) => {
            if node.is_null() {
                *node = Value::Object(Map::new());
            }
            let object = node
                .as_object_mut()
                .ok_or_else(|| format!("Expected object at --set field {field}"))?;
            assign(
                object.entry(field.clone()).or_insert(Value::Null),
                remaining,
                value,
            )
        }
        PathPart::Index(index) => {
            if node.is_null() {
                *node = Value::Array(Vec::new());
            }
            let array = node
                .as_array_mut()
                .ok_or("Expected array in Xray --set path")?;
            if *index > array.len() {
                return Err(format!(
                    "Xray --set array index {index} exceeds length {}",
                    array.len()
                ));
            }
            if *index == array.len() {
                array.push(Value::Null);
            }
            assign(&mut array[*index], remaining, value)
        }
    }
}

fn unique_tag(items: &[Value], base: &str) -> String {
    let mut tag = base.to_owned();
    let mut suffix = 1;
    while items
        .iter()
        .any(|item| item.get("tag").and_then(Value::as_str) == Some(tag.as_str()))
    {
        tag = format!("{base}-{suffix}");
        suffix += 1;
    }
    tag
}

pub(crate) fn configure(
    sets: Vec<String>,
    inbound_index: usize,
    applications: Option<&ApplicationRouting>,
    domains: Option<&DomainRouting>,
    proxy_tag: Option<&str>,
    match_processes: bool,
) -> Result<Vec<String>, String> {
    let mut config = json!({});
    for assignment in sets {
        let (path, raw) = assignment
            .split_once('=')
            .ok_or("Expected path=value in Xray sets")?;
        let value = serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.into()));
        assign(&mut config, &parse_path(path)?, value)?;
    }
    let (proxy_tag, direct_tag) = {
        let outbounds = config
            .get_mut("outbounds")
            .and_then(Value::as_array_mut)
            .ok_or("Application proxy mode requires Xray outbounds")?;
        if outbounds.is_empty() || outbounds.iter().any(|outbound| !outbound.is_object()) {
            return Err("Application proxy mode requires at least one valid outbound".into());
        }
        let proxy_tag = if let Some(tag) = proxy_tag {
            if !outbounds
                .iter()
                .any(|outbound| outbound["tag"].as_str() == Some(tag))
            {
                return Err(format!("Proxy outbound tag does not exist: {tag}"));
            }
            tag.to_owned()
        } else if let Some(tag) = outbounds[0]["tag"].as_str().filter(|tag| !tag.is_empty()) {
            tag.to_owned()
        } else {
            let tag = unique_tag(outbounds, "i-can-fly-proxy");
            outbounds[0]["tag"] = json!(tag);
            tag
        };
        let direct_tag = unique_tag(outbounds, "i-can-fly-direct");
        outbounds.push(json!({ "tag": direct_tag, "protocol": "freedom" }));
        (proxy_tag, direct_tag)
    };
    let inbound_tag = {
        let inbounds = config
            .as_object_mut()
            .ok_or("Xray config must be an object")?
            .entry("inbounds")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("Xray inbounds must be an array")?;
        if inbound_index > 1024 || inbound_index > inbounds.len() {
            return Err("TUN inbound index must refer to an existing inbound or append at the current length".into());
        }
        if inbound_index == inbounds.len() {
            inbounds.push(json!({}));
        }
        if !inbounds[inbound_index].is_object() {
            return Err("TUN inbound must be an object".into());
        }
        if domains.is_some() {
            let sniffing = inbounds[inbound_index]
                .as_object_mut()
                .ok_or("TUN inbound must be an object")?
                .entry("sniffing")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or("Inbound sniffing must be an object")?;
            sniffing.insert("enabled".into(), json!(true));
            sniffing.insert("routeOnly".into(), json!(true));
            sniffing.insert("metadataOnly".into(), json!(false));
            sniffing.insert("destOverride".into(), json!(["http", "tls", "quic"]));
        }
        if let Some(tag) = inbounds[inbound_index]["tag"]
            .as_str()
            .filter(|tag| !tag.is_empty())
        {
            tag.to_owned()
        } else {
            let tag = unique_tag(inbounds, "i-can-fly-tun");
            inbounds[inbound_index]["tag"] = json!(tag);
            tag
        }
    };
    let routing = config
        .as_object_mut()
        .ok_or("Xray config must be an object")?
        .entry("routing")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("Xray routing must be an object")?;
    let rules = routing
        .entry("rules")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("Xray routing.rules must be an array")?;
    // Android 应用筛选由 VpnService 完成；Windows 用 process 条件限定参与分流的应用。
    let applications = applications.filter(|_| match_processes);
    let processes: Option<Vec<String>> = applications.map(|policy| {
        policy
            .applications
            .iter()
            .map(|application| application.replace('\\', "/"))
            .collect()
    });
    let proxy_only_apps = applications.is_some_and(|policy| policy.mode == RoutingMode::Proxy);
    let mut generated = Vec::new();
    let rule = |tag: &str| {
        json!({
            "type": "field", "inboundTag": [inbound_tag], "network": "tcp,udp", "outboundTag": tag,
        })
    };
    if applications.is_some_and(|policy| policy.mode == RoutingMode::Bypass) {
        let mut bypass = rule(&direct_tag);
        bypass["process"] = json!(processes);
        generated.push(bypass);
    }
    if let Some(domains) = domains {
        let mut domain_rule = rule(if domains.mode == RoutingMode::Proxy {
            &proxy_tag
        } else {
            &direct_tag
        });
        domain_rule["domain"] = json!(domains.domains);
        if proxy_only_apps {
            domain_rule["process"] = json!(processes);
        }
        generated.push(domain_rule);
    }
    let default_tag = if domains.is_some_and(|policy| policy.mode == RoutingMode::Proxy) {
        &direct_tag
    } else {
        &proxy_tag
    };
    let mut fallback = rule(default_tag);
    if proxy_only_apps && default_tag == &proxy_tag {
        fallback["process"] = json!(processes);
        generated.push(fallback);
        generated.push(rule(&direct_tag));
    } else {
        generated.push(fallback);
    }
    // 新规则只处理该 TUN，其他入站仍使用原始路由。
    generated.append(rules);
    *rules = generated;
    Ok(config
        .as_object()
        .ok_or("Xray config must be an object")?
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect())
}
