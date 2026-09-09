use base64::{Engine, engine::general_purpose::STANDARD};
use url::Url;

use crate::ShopError;

pub(crate) struct Credentials {
    pub(crate) folder: String,
    pub(crate) api_key: String,
}

impl Credentials {
    pub(crate) fn parse(header: Option<&str>) -> Result<Self, ShopError> {
        let header = header
            .filter(|value| value.len() <= 8192)
            .ok_or(ShopError::Unauthorized)?;
        let (scheme, encoded) = header.split_once(' ').ok_or(ShopError::Unauthorized)?;
        if !scheme.eq_ignore_ascii_case("Basic") {
            return Err(ShopError::Unauthorized);
        }
        let bytes = STANDARD
            .decode(encoded.trim())
            .map_err(|_| ShopError::Unauthorized)?;
        let decoded = String::from_utf8(bytes).map_err(|_| ShopError::Unauthorized)?;
        let (username, api_key) = decoded.split_once(':').ok_or(ShopError::Unauthorized)?;
        if username.is_empty()
            || api_key.is_empty()
            || api_key.chars().any(char::is_whitespace)
            || api_key.chars().any(char::is_control)
            || !api_key.is_ascii()
        {
            return Err(ShopError::Unauthorized);
        }
        let folder = username.strip_prefix('/').unwrap_or(username);
        let folder = folder.strip_suffix('/').unwrap_or(folder);
        if (folder.is_empty() && username != "/")
            || folder
                .chars()
                .any(|character| character == '\\' || character.is_control())
            || (!folder.is_empty()
                && folder
                    .split('/')
                    .any(|segment| matches!(segment, "" | "." | "..")))
        {
            return Err(ShopError::InvalidFolder);
        }
        Ok(Self {
            folder: folder.to_owned(),
            api_key: api_key.to_owned(),
        })
    }

    pub(crate) fn folder_url(&self) -> Result<Url, ShopError> {
        let mut url = Url::parse("https://www.premiumize.me/api/folder/list")
            .map_err(|_| ShopError::Upstream)?;
        if !self.folder.is_empty() {
            url.query_pairs_mut().append_pair("path", &self.folder);
        }
        Ok(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authorization(value: &str) -> String {
        format!("Basic {}", STANDARD.encode(value))
    }

    #[test]
    fn basic_auth_maps_username_to_folder_and_preserves_password_colons() {
        let credentials =
            Credentials::parse(Some(&authorization("/Games/Switch/:key:part"))).unwrap();
        assert_eq!(
            (credentials.folder.as_str(), credentials.api_key.as_str()),
            ("Games/Switch", "key:part")
        );
    }

    #[test]
    fn root_is_explicit_and_omits_path_query() {
        let credentials = Credentials::parse(Some(&authorization("/:key"))).unwrap();
        assert_eq!(
            credentials.folder_url().unwrap().as_str(),
            "https://www.premiumize.me/api/folder/list"
        );
    }

    #[test]
    fn folder_names_are_encoded_once_as_query_data() {
        let folder = "Games/\u{00dc}ber + %2F #?&";
        let credentials =
            Credentials::parse(Some(&authorization(&format!("{folder}:key")))).unwrap();
        let url = credentials.folder_url().unwrap();
        assert_eq!(
            url.query_pairs().collect::<Vec<_>>(),
            vec![("path".into(), folder.into())]
        );
        assert!(!url.as_str().contains("key"));
    }

    #[test]
    fn scheme_is_case_insensitive() {
        assert!(
            Credentials::parse(Some(
                &authorization("Games:key").replacen("Basic", "bAsIc", 1)
            ))
            .is_ok()
        );
    }

    #[test]
    fn malformed_headers_are_rejected() {
        for header in [
            None,
            Some(""),
            Some("Basic !!!"),
            Some("Bearer abc"),
            Some("Basic /w=="),
        ] {
            assert!(matches!(
                Credentials::parse(header),
                Err(ShopError::Unauthorized)
            ));
        }
    }

    #[test]
    fn oversized_header_is_rejected_before_decoding() {
        assert!(matches!(
            Credentials::parse(Some(&"a".repeat(8193))),
            Err(ShopError::Unauthorized)
        ));
    }

    #[test]
    fn missing_or_unsafe_credentials_are_rejected() {
        for value in [
            "Games",
            ":key",
            "Games:",
            "Games:key\n",
            "Games:key part",
            "Games:\u{00fc}",
        ] {
            assert!(matches!(
                Credentials::parse(Some(&authorization(value))),
                Err(ShopError::Unauthorized)
            ));
        }
    }

    #[test]
    fn ambiguous_and_traversal_paths_are_rejected() {
        for folder in [
            "//",
            "//Games",
            "Games//",
            "Games//Switch",
            ".",
            "..",
            "Games/../Other",
            "Games/./Other",
            "Games\\Other",
            "Games\n",
        ] {
            assert!(
                matches!(
                    Credentials::parse(Some(&authorization(&format!("{folder}:key")))),
                    Err(ShopError::InvalidFolder)
                ),
                "{folder:?}"
            );
        }
    }
}
