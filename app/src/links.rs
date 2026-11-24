use std::collections::HashSet;

pub struct Links {
    pub urls: Vec<String>,
    pub invalid: usize,
    pub duplicates: usize,
}

pub fn parse_links(text: &str) -> Links {
    let mut result = Links {
        urls: Vec::new(),
        invalid: 0,
        duplicates: 0,
    };
    let mut seen = HashSet::new();
    for part in text.split(|c: char| c.is_whitespace() || c == ',') {
        let token = part.trim_matches(['<', '>', '(', ')', '[', ']', '"', '\'']);
        if token.is_empty() {
            continue;
        }
        match video_id(token) {
            Some(id) if seen.insert(id) => result
                .urls
                .push(format!("https://www.youtube.com/watch?v={id}")),
            Some(_) => result.duplicates += 1,
            None => result.invalid += 1,
        }
    }
    result
}

/// The video ID of a supported YouTube link.
pub fn video_id(token: &str) -> Option<&str> {
    let rest = token
        .strip_prefix("https://")
        .or_else(|| token.strip_prefix("http://"))
        .unwrap_or(token);
    let (host, path) = rest.split_once('/')?;
    let (path, query) = path
        .split('#')
        .next()?
        .split_once('?')
        .unwrap_or((path, ""));
    let id = match host.to_ascii_lowercase().as_str() {
        "youtu.be" => path,
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            if path == "watch" {
                query
                    .split('&')
                    .find_map(|parameter| parameter.strip_prefix("v="))?
            } else {
                let (kind, id) = path.split_once('/')?;
                match kind {
                    "shorts" | "live" | "embed" | "v" => id,
                    _ => return None,
                }
            }
        }
        _ => return None,
    };
    (id.len() == 11
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    .then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "dQw4w9WgXcQ";

    #[test]
    fn video_id_accepts_every_supported_link() {
        for link in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "http://youtube.com/watch?v=dQw4w9WgXcQ",
            "youtube.com/watch?v=dQw4w9WgXcQ",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=abc",
            "https://WWW.YouTube.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/watch?list=PL123&v=dQw4w9WgXcQ&t=5s",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ#comments",
            "https://youtu.be/dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?t=10",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/live/dQw4w9WgXcQ?feature=share",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "https://www.youtube.com/v/dQw4w9WgXcQ",
        ] {
            assert_eq!(video_id(link), Some(ID), "{link}");
        }
    }

    #[test]
    fn video_id_rejects_other_links() {
        for link in [
            "",
            "dQw4w9WgXcQ",
            "https://www.youtube.com",
            "https://www.youtube.com/",
            "https://www.youtube.com/watch",
            "https://www.youtube.com/watch?list=PL123",
            "https://www.youtube.com/playlist?list=PL123",
            "https://www.youtube.com/channel/dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=tooshort",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQQ",
            "https://youtu.be/dQw4w9WgXc!",
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com.example.com/watch?v=dQw4w9WgXcQ",
            "ftp://youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            assert_eq!(video_id(link), None, "{link}");
        }
    }

    #[test]
    fn parse_links_of_empty_text_finds_nothing() {
        for text in ["", " \n\t , ", "<>"] {
            let links = parse_links(text);
            assert!(links.urls.is_empty());
            assert_eq!((links.invalid, links.duplicates), (0, 0));
        }
    }

    #[test]
    fn parse_links_splits_and_normalizes_in_order() {
        let links = parse_links(
            "https://youtu.be/aaaaaaaaaaa, <https://youtu.be/bbbbbbbbbbb>\n\"youtube.com/shorts/ccccccccccc\"\t(https://youtu.be/d-d_ddddddd)",
        );
        assert_eq!(
            links.urls,
            [
                "https://www.youtube.com/watch?v=aaaaaaaaaaa",
                "https://www.youtube.com/watch?v=bbbbbbbbbbb",
                "https://www.youtube.com/watch?v=ccccccccccc",
                "https://www.youtube.com/watch?v=d-d_ddddddd",
            ]
        );
        assert_eq!((links.invalid, links.duplicates), (0, 0));
    }

    #[test]
    fn parse_links_counts_invalid_and_duplicate_links() {
        let links = parse_links(
            "https://youtu.be/dQw4w9WgXcQ https://www.youtube.com/watch?v=dQw4w9WgXcQ hello https://example.com/",
        );
        assert_eq!(links.urls, ["https://www.youtube.com/watch?v=dQw4w9WgXcQ"]);
        assert_eq!((links.invalid, links.duplicates), (2, 1));
    }
}
