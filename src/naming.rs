use chrono::{DateTime, NaiveDateTime, Utc};
use chrono_tz::Tz;

// "__" separates hierarchy levels and "--" separates the hierarchy chain from
// the timestamp, so names must not contain either. Kept deliberately simple:
// a full slugify would be more robust but names here are folder/#directive
// derived, not user free text.
fn sanitize(part: &str) -> String {
    part.replace("__", "_")
        .replace("--", "-")
        .replace(['/', '\\', ' ', '.'], "-")
}

/// `server__chain--YYYY-MM-DD-HHMM-TZ.ext`. Snapshots use `zip`; streamed
/// uploads (`put`) use the extension of whatever they carry (`dump`, `tar`).
pub fn object_name(server: &str, chain: &[String], tz: Tz, ext: &str) -> String {
    let now = Utc::now().with_timezone(&tz);
    let stamp = now.format("%Y-%m-%d-%H%M");
    let tz_abbr = now.format("%Z");
    let parts: Vec<String> = std::iter::once(server.to_string())
        .chain(chain.iter().cloned())
        .map(|p| sanitize(&p))
        .collect();
    format!("{}--{}-{}.{}", parts.join("__"), stamp, tz_abbr, sanitize(ext))
}

pub fn archive_filename(server: &str, chain: &[String], tz: Tz) -> String {
    object_name(server, chain, tz, "zip")
}

pub struct ParsedName {
    pub server: String,
    pub chain: Vec<String>,
    pub timestamp: String,
    pub ext: String,
}

impl ParsedName {
    /// The timestamp is written in the configured zone; the trailing zone
    /// abbreviation is informational only.
    pub fn time(&self, tz: Tz) -> Option<DateTime<Utc>> {
        let local = self.timestamp.get(..15)?;
        NaiveDateTime::parse_from_str(local, "%Y-%m-%d-%H%M")
            .ok()?
            .and_local_timezone(tz)
            .earliest()
            .map(|t| t.with_timezone(&Utc))
    }
}

pub fn parse_archive_filename(filename: &str) -> Option<ParsedName> {
    let (chain_part, rest) = filename.rsplit_once("--")?;
    let (timestamp, ext) = rest.split_once('.')?;
    let mut parts = chain_part.split("__").map(|s| s.to_string());
    let server = parts.next()?;
    let chain: Vec<String> = parts.collect();
    Some(ParsedName {
        server,
        chain,
        timestamp: timestamp.to_string(),
        ext: ext.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let name = archive_filename(
            "server1",
            &["project1".to_string(), "sub-project-a".to_string()],
            chrono_tz::Europe::Helsinki,
        );
        let parsed = parse_archive_filename(&name).unwrap();
        assert_eq!(parsed.server, "server1");
        assert_eq!(parsed.chain, vec!["project1", "sub-project-a"]);
        assert_eq!(parsed.ext, "zip");
        assert!(parsed.time(chrono_tz::Europe::Helsinki).is_some());
    }

    #[test]
    fn stream_objects_keep_their_extension() {
        let name = object_name("server1", &["dropcat-db".to_string()], chrono_tz::Europe::Moscow, "dump");
        let parsed = parse_archive_filename(&name).unwrap();
        assert_eq!(parsed.chain, vec!["dropcat-db"]);
        assert_eq!(parsed.ext, "dump");
        let fixed = parse_archive_filename("server1__srv--2026-07-19-1657-MSK.zip").unwrap();
        let t = fixed.time(chrono_tz::Europe::Moscow).unwrap();
        assert_eq!(t.to_rfc3339(), "2026-07-19T13:57:00+00:00");
    }
}
