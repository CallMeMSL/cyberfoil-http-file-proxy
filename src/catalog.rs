use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Deserialize)]
pub(crate) struct FolderResponse {
    status: Status,
    content: Option<Vec<Entry>>,
    code: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Success,
    Error,
}

#[derive(Deserialize)]
pub(crate) struct Entry {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    size: Option<u64>,
    link: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Catalog {
    sections: Vec<Section>,
}

#[derive(Debug, Serialize)]
struct Section {
    id: &'static str,
    title: String,
    items: Vec<Item>,
}

#[derive(Debug, Serialize)]
struct Item {
    name: String,
    size: u64,
    url: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CatalogError {
    InvalidItem,
    Upstream(String),
}

impl FolderResponse {
    pub(crate) fn into_catalog(self, folder: &str) -> Result<Catalog, CatalogError> {
        let entries = match self.status {
            Status::Success => self.content.ok_or(CatalogError::InvalidItem)?,
            Status::Error => return Err(CatalogError::Upstream(self.code.unwrap_or_default())),
        };
        let items = entries
            .into_iter()
            .filter(|entry| entry.kind == "file" && is_installable(&entry.name))
            .map(|entry| {
                let link = entry.link.ok_or(CatalogError::InvalidItem)?;
                let parsed = Url::parse(&link).map_err(|_| CatalogError::InvalidItem)?;
                if parsed.scheme() != "https"
                    || parsed.host_str().is_none()
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || link
                        .chars()
                        .any(|character| character.is_control() || character.is_whitespace())
                {
                    return Err(CatalogError::InvalidItem);
                }
                Ok(Item {
                    name: entry.name,
                    size: entry.size.ok_or(CatalogError::InvalidItem)?,
                    url: link,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Catalog {
            sections: vec![Section {
                id: "premiumize",
                title: if folder.is_empty() {
                    "Premiumize"
                } else {
                    folder
                }
                .to_owned(),
                items,
            }],
        })
    }
}

fn is_installable(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty()
            && ["nsp", "nsz", "xci", "xcz"]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalog_preserves_direct_links_and_sizes_and_ignores_subfolders() {
        let response: FolderResponse = serde_json::from_value(json!({
            "status": "success",
            "content": [
                {"type": "folder", "name": "Nested.nsp"},
                {"type": "file", "name": "Notes.txt"},
                {"type": "file", "name": "Example.NSP", "size": 5000000000u64,
                 "link": "https://cdn.premiumize.me/a%2Fb.nsp?token=abc%2Fdef&expires=123"}
            ]
        }))
        .unwrap();
        let catalog = response.into_catalog("Games/Switch").unwrap();
        assert_eq!(
            serde_json::to_value(catalog).unwrap(),
            json!({
                "sections": [{"id": "premiumize", "title": "Games/Switch", "items": [
                    {"name": "Example.NSP", "size": 5000000000u64,
                     "url": "https://cdn.premiumize.me/a%2Fb.nsp?token=abc%2Fdef&expires=123"}
                ]}]
            })
        );
    }

    #[test]
    fn empty_root_has_an_empty_section() {
        let response = FolderResponse {
            status: Status::Success,
            content: Some(vec![]),
            code: None,
        };
        assert_eq!(
            serde_json::to_value(response.into_catalog("").unwrap()).unwrap(),
            json!({"sections": [{"id": "premiumize", "title": "Premiumize", "items": []}]})
        );
    }

    #[test]
    fn api_error_is_not_an_empty_catalog() {
        let response: FolderResponse = serde_json::from_str(
            r#"{"status":"error","code":"authentication_failed","message":"secret"}"#,
        )
        .unwrap();
        assert_eq!(
            response.into_catalog("Games").unwrap_err(),
            CatalogError::Upstream("authentication_failed".to_owned())
        );
    }

    #[test]
    fn supported_extensions_are_case_insensitive() {
        for name in ["Game.nsp", "Game.NSZ", "Game.Xci", "Game.xCZ"] {
            assert!(is_installable(name), "{name}");
        }
    }

    #[test]
    fn unrelated_files_are_not_installable() {
        for name in ["Game.zip", "Game.nsp.txt", "Game", ".nsp"] {
            assert!(!is_installable(name), "{name}");
        }
    }

    #[test]
    fn invalid_links_fail_instead_of_returning_broken_items() {
        for link in [
            "",
            "/relative.nsp",
            "http://cdn.example/game.nsp",
            "https://user:key@cdn.example/a.nsp",
            "https://cdn.example/a\nnsp",
            " https://cdn.example/a.nsp",
            "https://cdn.example/a b.nsp",
        ] {
            let response = FolderResponse {
                status: Status::Success,
                content: Some(vec![Entry {
                    kind: "file".to_owned(),
                    name: "Game.nsp".to_owned(),
                    size: Some(1),
                    link: Some(link.to_owned()),
                }]),
                code: None,
            };
            assert_eq!(
                response.into_catalog("Games").unwrap_err(),
                CatalogError::InvalidItem
            );
        }
    }
}
