//! Origin-confined, memory-only request context. No cookie jar or response secrets.

use std::fmt;
use std::sync::Arc;

use download_manager_protocol::{CookieInput, RequestContextInput};
use reqwest::Url;
use reqwest::header::{AUTHORIZATION, COOKIE, HeaderMap, HeaderValue, REFERER};
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

/// Bounded failure with no supplied data in its representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum ContextError {
    /// A supplied field or scope is unsafe.
    #[error("request context is invalid or outside its permitted origin")]
    Invalid,
    /// Cookie lifetime elapsed; never retry with a silently reduced session.
    #[error("session expired; sign in and explicitly add a fresh download")]
    Expired,
}

#[derive(Clone, Eq, PartialEq)]
struct Cookie {
    name: String,
    value: String,
    domain: String,
    path: String,
    secure: bool,
    expires: Option<OffsetDateTime>,
}

/// Validated secrets scoped to one exact origin. Intentionally not serializable.
#[derive(Clone, Eq, PartialEq)]
pub struct RequestContext {
    origin: String,
    cookies: Vec<Cookie>,
    authorization: Option<HeaderValue>,
    referrer: Option<HeaderValue>,
}

impl fmt::Debug for RequestContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RequestContext(<redacted>)")
    }
}

impl RequestContext {
    /// Independently validates input from the protocol boundary.
    ///
    /// # Errors
    /// Rejects injection, excessive headers, inapplicable cookies, insecure
    /// authorization, expired cookies, and cross-origin referrers.
    pub fn new(target: &str, input: &RequestContextInput) -> Result<Arc<Self>, ContextError> {
        if !input.is_valid() {
            return Err(ContextError::Invalid);
        }
        let url = safe_url(target)?;
        let referrer = input
            .referrer
            .as_ref()
            .map(|value| {
                let parsed = safe_url(value)?;
                if parsed.origin() != url.origin() || parsed.fragment().is_some() {
                    return Err(ContextError::Invalid);
                }
                sensitive(value)
            })
            .transpose()?;
        let mut context = Self {
            origin: url.origin().ascii_serialization(),
            cookies: Vec::new(),
            authorization: None,
            referrer,
        };
        if let Some(credentials) = &input.credentials {
            if let Some(authorization) = &credentials.authorization {
                if url.scheme() != "https"
                    || !matches!(authorization.scheme.as_str(), "Basic" | "Bearer")
                    || authorization.value.is_empty()
                    || authorization
                        .value
                        .bytes()
                        .any(|b| !(0x21..=0x7e).contains(&b))
                {
                    return Err(ContextError::Invalid);
                }
                context.authorization = Some(sensitive(&format!(
                    "{} {}",
                    authorization.scheme, authorization.value
                ))?);
            }
            if let Some(cookies) = &credentials.cookies {
                if cookies.len() > 256 {
                    return Err(ContextError::Invalid);
                }
                for input in cookies {
                    let cookie = Cookie::new(input)?;
                    if !cookie.applies(&url) {
                        return Err(ContextError::Invalid);
                    }
                    context.cookies.push(cookie);
                }
                // Longest path first, preserving equal-path input order.
                context
                    .cookies
                    .sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));
            }
        }
        context.headers(&url)?;
        Ok(Arc::new(context))
    }

    /// Re-evaluates scope and expiry before every request, including redirects.
    ///
    /// # Errors
    /// Rejects a different origin or an expired session. Path-inapplicable
    /// cookies are omitted on same-origin redirects; no new cookies are read.
    pub fn headers(&self, url: &Url) -> Result<HeaderMap, ContextError> {
        self.headers_at(url, OffsetDateTime::now_utc())
    }

    fn headers_at(&self, url: &Url, now: OffsetDateTime) -> Result<HeaderMap, ContextError> {
        if url.origin().ascii_serialization() != self.origin {
            return Err(ContextError::Invalid);
        }
        let mut headers = HeaderMap::new();
        if let Some(value) = &self.authorization {
            headers.insert(AUTHORIZATION, value.clone());
        }
        if let Some(value) = &self.referrer {
            headers.insert(REFERER, value.clone());
        }
        let mut cookie_header = String::new();
        for cookie in &self.cookies {
            if cookie.expires.is_some_and(|expires| expires <= now) {
                return Err(ContextError::Expired);
            }
            if cookie.applies(url) {
                if !cookie_header.is_empty() {
                    cookie_header.push_str("; ");
                }
                cookie_header.push_str(&cookie.name);
                cookie_header.push('=');
                cookie_header.push_str(&cookie.value);
            }
        }
        if !cookie_header.is_empty() {
            headers.insert(COOKIE, sensitive(&cookie_header)?);
        }
        if headers.values().map(HeaderValue::len).sum::<usize>() > 8 * 1024 {
            return Err(ContextError::Invalid);
        }
        Ok(headers)
    }
}

impl Cookie {
    fn new(input: &CookieInput) -> Result<Self, ContextError> {
        if input.name.is_empty()
            || !input.name.bytes().all(token_byte)
            || !input
                .value
                .bytes()
                .all(|b| matches!(b, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e))
            || !input.path.starts_with('/')
            || input.path.bytes().any(|b| b < 0x21 || b == 0x7f)
            || input.domain.is_empty()
            || !input.domain.is_ascii()
            || input.domain.bytes().any(|b| {
                !b.is_ascii_alphanumeric() && !matches!(b, b'.' | b'-' | b':' | b'[' | b']')
            })
        {
            return Err(ContextError::Invalid);
        }
        let expires = input
            .expires_at()
            .map(|value| OffsetDateTime::parse(value, &Rfc3339).map_err(|_| ContextError::Invalid))
            .transpose()?;
        Ok(Self {
            name: input.name.clone(),
            value: input.value.clone(),
            domain: input.domain.to_ascii_lowercase(),
            path: input.path.clone(),
            secure: input.secure,
            expires,
        })
    }

    fn applies(&self, url: &Url) -> bool {
        let host = url.host_str().unwrap_or_default();
        let domain_ok = self.domain.strip_prefix('.').map_or_else(
            || host == self.domain,
            |domain| {
                !domain.is_empty() && (host == domain || host.ends_with(&format!(".{domain}")))
            },
        );
        let path_ok = url.path() == self.path
            || url
                .path()
                .strip_prefix(&self.path)
                .is_some_and(|suffix| self.path.ends_with('/') || suffix.starts_with('/'));
        domain_ok && path_ok && (!self.secure || url.scheme() == "https")
    }
}

fn safe_url(value: &str) -> Result<Url, ContextError> {
    let url = Url::parse(value).map_err(|_| ContextError::Invalid)?;
    if value.len() > 16_384
        || value.bytes().any(|b| b <= 0x20 || b == 0x7f)
        || !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ContextError::Invalid);
    }
    Ok(url)
}

fn sensitive(value: &str) -> Result<HeaderValue, ContextError> {
    if value.len() > 8 * 1024 || value.bytes().any(|b| !(0x20..=0x7e).contains(&b)) {
        return Err(ContextError::Invalid);
    }
    let mut header = HeaderValue::from_str(value).map_err(|_| ContextError::Invalid)?;
    header.set_sensitive(true);
    Ok(header)
}

fn token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_expiry_is_rechecked_on_every_send_with_exact_utc_boundary() {
        let input = serde_json::from_value(serde_json::json!({"credentials":{"cookies":[{
            "name":"fixture","value":"synthetic","domain":"example.test","path":"/","secure":true,
            "http_only":true,"expires_at":"2099-01-01T02:00:00+02:00"
        }]}}))
        .expect("input");
        let target = Url::parse("https://example.test/file").expect("URL");
        let context = RequestContext::new(target.as_str(), &input).expect("valid lifetime");
        let expiry = OffsetDateTime::parse("2099-01-01T00:00:00Z", &Rfc3339).expect("UTC expiry");
        assert!(
            context
                .headers_at(&target, expiry - time::Duration::nanoseconds(1))
                .is_ok()
        );
        assert_eq!(
            context.headers_at(&target, expiry),
            Err(ContextError::Expired)
        );
        assert_eq!(
            context.headers_at(&target, expiry + time::Duration::days(1)),
            Err(ContextError::Expired)
        );
    }
}
