use chrono::Utc;
use chrono_tz::Tz;

// "__" separates hierarchy levels and "--" separates the hierarchy chain from
// the timestamp, so names must not contain either. Kept deliberately simple:
// a full slugify would be more robust but names here are folder/#directive
// derived, not user free text.
fn sanitize(part: &str) -> String {
    part.replace("__", "_")
        .replace("--", "-")
        .replace(['/', '\\', ' '], "-")
}

pub fn archive_filename(server: &str, chain: &[String], tz: Tz) -> String {
    let now = Utc::now().with_timezone(&tz);
    let stamp = now.format("%Y-%m-%d-%H%M");
    let tz_abbr = now.format("%Z");
    let parts: Vec<String> = std::iter::once(server.to_string())
        .chain(chain.iter().cloned())
        .map(|p| sanitize(&p))
        .collect();
    format!("{}--{}-{}.zip", parts.join("__"), stamp, tz_abbr)
}

pub struct ParsedName {
    pub server: String,
    pub chain: Vec<String>,
    pub timestamp: String,
}

pub fn parse_archive_filename(filename: &str) -> Option<ParsedName> {
    let stem = filename.strip_suffix(".zip")?;
    let (chain_part, timestamp) = stem.rsplit_once("--")?;
    let mut parts = chain_part.split("__").map(|s| s.to_string());
    let server = parts.next()?;
    let chain: Vec<String> = parts.collect();
    Some(ParsedName {
        server,
        chain,
        timestamp: timestamp.to_string(),
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
    }
}
