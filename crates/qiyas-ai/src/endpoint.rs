//! Validated credential destinations. The complete base path is part of the identity.
pub fn normalize(base: &str) -> Result<String, String> {
    let base = base.trim();
    if base.contains(char::is_whitespace)
        || base.contains('\\')
        || !base.contains("://")
        || base
            .split("://")
            .nth(1)
            .and_then(|s| s.split('/').next())
            .is_some_and(|s| s.ends_with(':'))
    {
        return Err("Enter a complete HTTPS service address".into());
    }
    let url = reqwest::Url::parse(base).map_err(|_| "Enter a complete HTTPS service address")?;
    let host = url
        .host_str()
        .ok_or("Enter a complete HTTPS service address")?;
    let ip = host
        .trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
        .ok();
    let loopback = host == "localhost" || ip.is_some_and(|ip| ip.is_loopback());
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err("Use HTTPS for remote services; HTTP is allowed only on loopback".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || (!loopback
            && ip.is_none()
            && (host.len() > 253
                || !host.contains('.')
                || host.ends_with('.')
                || host.split('.').any(|p| {
                    p.is_empty()
                        || p.len() > 63
                        || p.starts_with('-')
                        || p.ends_with('-')
                        || !p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })))
    {
        return Err(
            "Enter a complete service address without credentials, query or fragment".into(),
        );
    }
    Ok(url.to_string().trim_end_matches('/').to_string())
}
