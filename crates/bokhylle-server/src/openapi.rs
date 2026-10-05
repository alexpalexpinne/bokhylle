//! Completes the response and security information that handler signatures
//! cannot express (variable status codes, session cookies, and file bodies).

use std::collections::{BTreeSet, VecDeque};

use serde_json::{Value, json};

pub fn complete(document: aide::openapi::OpenApi) -> Value {
    let mut document = serde_json::to_value(document).expect("OpenAPI document serializes");
    document["components"]["securitySchemes"] = json!({
        "sessionCookie": {
            "type": "apiKey",
            "in": "cookie",
            "name": crate::auth::SESSION_COOKIE,
            "description": "Session cookie returned by login. Admin and child permissions are enforced by the server."
        }
    });

    let paths = document["paths"]
        .as_object_mut()
        .expect("OpenAPI has paths");
    for (path, item) in paths {
        let operations = item.as_object_mut().expect("path item is an object");
        for (method, operation) in operations {
            let operation_id = format!(
                "{}_{}",
                method,
                path.trim_start_matches("/api/")
                    .replace(['/', '{', '}', '-'], "_")
                    .trim_matches('_')
            );
            operation["operationId"] = json!(operation_id);
            let tag = path
                .trim_start_matches("/api/")
                .split('/')
                .next()
                .unwrap_or("api");
            operation["tags"] = json!([tag]);

            if is_public(method, path) {
                operation["security"] = json!([]);
            } else {
                operation["security"] = json!([{ "sessionCookie": [] }]);
            }

            add_path_parameters(path, operation);
            normalize_query_parameters(operation);
            if method == "put" && path == "/api/profile/avatar" {
                let binary = json!({"schema": {"type": "string", "format": "binary"}});
                operation["requestBody"] = json!({
                    "required": true,
                    "content": {
                        "image/png": binary.clone(),
                        "image/jpeg": binary.clone(),
                        "image/webp": binary
                    }
                });
            }

            if !operation["responses"].is_object() {
                operation["responses"] = json!({});
            }
            let responses = operation["responses"]
                .as_object_mut()
                .expect("responses object");
            if !responses.keys().any(|code| code.starts_with('2')) {
                for (code, response) in success_responses(method, path) {
                    responses.insert(code.to_string(), response);
                }
            }
            responses.entry("default").or_insert_with(|| json!({
                "description": "API error. HTTP status and code identify the failure.",
                "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ErrorBody"}}}
            }));
            let error = responses["default"].clone();
            if !is_public(method, path) {
                responses.entry("401").or_insert_with(|| error.clone());
                responses.entry("403").or_insert_with(|| error.clone());
            }
            if path == "/api/auth/login"
                || path == "/api/demo/enter"
                || path == "/api/demo/switch"
                || path == "/api/profile/credential"
                || path == "/api/auth/logout"
                || path == "/api/auth/logout-all"
            {
                for (code, response) in responses
                    .iter_mut()
                    .filter(|(code, _)| code.starts_with('2'))
                {
                    let _ = code;
                    response["headers"] = json!({
                        "Set-Cookie": {"description": "Sets or clears the session cookie", "schema": {"type": "string"}}
                    });
                }
            }
        }
    }
    // Omitted correction fields retain their values. Schema defaults would
    // make generated clients populate (and require) nullable patch fields.
    let correction = &mut document["components"]["schemas"]["BookUpdateInput"];
    correction["required"] = json!([]);
    if let Some(properties) = correction["properties"].as_object_mut() {
        for property in properties.values_mut() {
            if let Some(property) = property.as_object_mut() {
                property.remove("default");
            }
        }
    }
    require_serialized_response_fields(&mut document);
    document
}

/// Schemars treats `Option<T>` as optional for deserialization. Response structs
/// serialize those fields as explicit null unless serde skips them, so describe
/// the wire shape instead of the input shape. Request and response components
/// are distinct in this API.
fn require_serialized_response_fields(document: &mut Value) {
    let mut queue = VecDeque::new();
    for item in document["paths"]
        .as_object()
        .into_iter()
        .flat_map(|paths| paths.values())
    {
        for operation in item.as_object().into_iter().flat_map(|item| item.values()) {
            for (status, response) in operation["responses"]
                .as_object()
                .into_iter()
                .flat_map(|responses| responses.iter())
            {
                if status.starts_with('2') {
                    collect_schema_refs(response, &mut queue);
                }
            }
        }
    }

    let mut seen = BTreeSet::new();
    while let Some(name) = queue.pop_front() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let schema = &mut document["components"]["schemas"][&name];
        collect_schema_refs(schema, &mut queue);
        let Some(properties) = schema["properties"].as_object() else {
            continue;
        };
        let mut required: BTreeSet<String> = schema["required"]
            .as_array()
            .into_iter()
            .flat_map(|required| required.iter())
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        for property in properties.keys() {
            if !skipped_response_property(&name, property) {
                required.insert(property.clone());
            }
        }
        schema["required"] = json!(required);
    }
}

fn collect_schema_refs(value: &Value, queue: &mut VecDeque<String>) {
    match value {
        Value::Object(object) => {
            if let Some(name) = object
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|reference| reference.strip_prefix("#/components/schemas/"))
            {
                queue.push_back(name.to_owned());
            }
            for nested in object.values() {
                collect_schema_refs(nested, queue);
            }
        }
        Value::Array(array) => {
            for nested in array {
                collect_schema_refs(nested, queue);
            }
        }
        _ => {}
    }
}

fn skipped_response_property(schema: &str, property: &str) -> bool {
    match schema {
        "DownloadStatus" => property == "downloadsDir",
        "ReleaseView" => matches!(property, "releaseName" | "leechers" | "indexer"),
        "SpotlightItem" => matches!(
            property,
            "languages"
                | "rating"
                | "ratingCount"
                | "ratingSource"
                | "bookId"
                | "provider"
                | "providerKey"
                | "coverId"
                | "coverProvider"
                | "year"
        ),
        "CandidateView" => matches!(property, "score" | "scoreReasons" | "rejectionReasons"),
        "ReleaseCandidate" => matches!(
            property,
            "detectedTitle"
                | "detectedAuthor"
                | "detectedFormat"
                | "detectedLanguage"
                | "detectedVolume"
                | "isCollection"
                | "isAudiobook"
                | "isComic"
        ),
        _ => false,
    }
}

fn is_public(method: &str, path: &str) -> bool {
    matches!(
        (method, path),
        ("get", "/api/health")
            | ("post", "/api/auth/login")
            | ("post", "/api/auth/logout")
            | ("get", "/api/auth/users")
            | ("get", "/api/auth/users/{id}/avatar")
            | ("get", "/api/demo")
            | ("post", "/api/demo/enter")
    )
}

fn add_path_parameters(path: &str, operation: &mut Value) {
    for part in path.split('/') {
        let Some(name) = part
            .strip_prefix('{')
            .and_then(|part| part.strip_suffix('}'))
        else {
            continue;
        };
        let is_string = matches!(name, "key" | "normalized" | "cover_id")
            || name == "id"
                && (path.starts_with("/api/acquisitions/")
                    || path.starts_with("/api/admin/acquisitions/")
                    || path.starts_with("/api/catalogues/"));
        let schema = if is_string {
            json!({"type": "string"})
        } else {
            json!({"type": "integer", "format": "int64"})
        };
        let parameters = operation["parameters"].as_array_mut();
        if let Some(parameters) = parameters {
            if !parameters
                .iter()
                .any(|parameter| parameter["in"] == "path" && parameter["name"] == name)
            {
                parameters
                    .push(json!({"name": name, "in": "path", "required": true, "schema": schema}));
            }
        } else {
            operation["parameters"] =
                json!([{"name": name, "in": "path", "required": true, "schema": schema}]);
        }
    }
}

fn normalize_query_parameters(operation: &mut Value) {
    let Some(parameters) = operation
        .get_mut("parameters")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for parameter in parameters {
        if parameter["in"] != "query" {
            continue;
        }
        let Some(types) = parameter["schema"]["type"].as_array() else {
            continue;
        };
        let concrete = types
            .iter()
            .find(|kind| kind.as_str() != Some("null"))
            .cloned();
        if let Some(concrete) = concrete {
            parameter["schema"]["type"] = concrete;
        }
    }
}

fn success_responses(method: &str, path: &str) -> Vec<(u16, Value)> {
    use Success as S;
    let entries: &[S] = match (method, path) {
        ("post", "/api/auth/login" | "/api/demo/enter" | "/api/demo/switch")
        | ("put", "/api/profile/credential") => &[S::Json(200, "MeResponse")],
        ("put", "/api/books/{id}/sharing") => &[S::Json(200, "BookSharingState")],
        ("post", "/api/recommendations/impressions")
        | (
            "delete",
            "/api/profile/rejected/{id}" | "/api/recommendations/{key}/feedback/{token}",
        ) => &[S::Empty(204)],
        ("put", "/api/books/sharing") => &[S::Empty(204)],
        ("post", "/api/auth/logout" | "/api/auth/logout-all") => &[S::Empty(204)],
        ("get", "/api/profile/avatar") => {
            &[S::Binary(200, &["image/png", "image/jpeg", "image/webp"])]
        }
        ("get", "/api/auth/users/{id}/avatar") => &[S::Binary(200, &["image/png"])],
        ("put" | "delete", "/api/profile/avatar") => &[S::Empty(204)],
        ("post", "/api/admin/users") => &[S::Json(201, "AdminUserView")],
        ("post", "/api/collections") => &[S::Json(201, "CollectionSummary")],
        ("post", "/api/delivery-targets") => &[S::Json(201, "DeliveryTarget")],
        ("post", "/api/admin/children/{id}/readers") => &[S::Json(201, "DeliveryTarget")],
        ("post", "/api/admin/maintenance/images") => &[S::Json(202, "ImageJobStatus")],
        ("post", "/api/admin/maintenance/metadata") => &[S::Json(202, "MetadataJobStatus")],
        ("post", "/api/admin/maintenance/imports") => &[S::Json(202, "ImportJobStatus")],
        ("post", "/api/books/{book_id}/files/{file_id}/deliver")
        | ("post", "/api/deliveries/{id}/retry") => &[S::Json(202, "Delivery")],
        ("post", "/api/admin/maintenance/metadata/cancel") => &[S::Empty(202)],
        ("get", "/api/admin/backup") => &[S::Binary(200, &["application/octet-stream"])],
        ("get", "/api/books/{book_id}/files/{file_id}/download") => &[S::Binary(
            200,
            &[
                "application/epub+zip",
                "application/pdf",
                "application/octet-stream",
            ],
        )],
        ("get", "/api/books/{book_id}/files/{file_id}/content") => &[S::Binary(
            200,
            &[
                "application/epub+zip",
                "application/pdf",
                "application/vnd.comicbook+zip",
            ],
        )],
        ("get", "/api/books/{book_id}/files/{file_id}/pages") => &[S::Json(200, "ComicManifest")],
        ("get", "/api/books/{book_id}/files/{file_id}/pages/{page}") => &[S::Binary(
            200,
            &["image/jpeg", "image/png", "image/webp", "image/gif"],
        )],
        ("get" | "put", "/api/books/{book_id}/files/{file_id}/position") => {
            &[S::Json(200, "BrowserPositionState")]
        }
        ("get", "/api/books/{id}/cover" | "/api/discover/cover/{cover_id}") => &[S::Binary(
            200,
            &[
                "image/jpeg",
                "image/png",
                "image/gif",
                "image/webp",
                "image/bmp",
                "image/svg+xml",
                "application/octet-stream",
            ],
        )],
        ("get", "/api/authors/{id}/photo" | "/api/discover/authors/photo") => {
            &[S::Binary(200, &["image/jpeg"]), S::Empty(404)]
        }
        ("delete", "/api/admin/books/{id}" | "/api/admin/books/{book_id}/files/{file_id}")
        | ("delete", "/api/catalogues/{id}")
        | (
            "delete",
            "/api/admin/children/{id}/readers/{target_id}"
            | "/api/admin/children/{id}/reader-tokens/{token_id}",
        )
        | ("delete", "/api/profile/tokens/{id}" | "/api/profile/agent-tokens/{id}")
        | ("delete", "/api/collections/{id}" | "/api/collections/{id}/books/{book_id}")
        | (
            "delete",
            "/api/delivery-targets/{id}" | "/api/books/{id}/shelf" | "/api/authors/{id}/follow",
        )
        | ("put", "/api/admin/users/{id}/profile-type" | "/api/users/{id}/shelf/{book_id}")
        | (
            "put",
            "/api/books/{id}/shelf"
            | "/api/books/{id}/preference"
            | "/api/home/subjects/{normalized}",
        )
        | ("put", "/api/authors/{id}/automation")
        | ("post", "/api/admin/users/{id}/restart-onboarding" | "/api/profile/onboarded")
        | ("post", "/api/collections/{id}/books" | "/api/authors/{id}/follow") => &[S::Empty(204)],
        _ => panic!("OpenAPI success response missing for {method} {path}"),
    };
    entries.iter().map(|entry| entry.response()).collect()
}

enum Success {
    Empty(u16),
    Json(u16, &'static str),
    Binary(u16, &'static [&'static str]),
}

impl Success {
    fn response(&self) -> (u16, Value) {
        match self {
            Self::Empty(status) => (*status, json!({"description": "Success"})),
            Self::Json(status, name) => {
                let schema = json!({"$ref": format!("#/components/schemas/{name}")});
                (
                    *status,
                    json!({"description": "Success", "content": {"application/json": {"schema": schema}}}),
                )
            }
            Self::Binary(status, types) => {
                let mut content = serde_json::Map::new();
                for media_type in *types {
                    content.insert(
                        (*media_type).to_string(),
                        json!({"schema": {"type": "string", "format": "binary"}}),
                    );
                }
                (
                    *status,
                    json!({"description": "Binary response", "content": content}),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn response_fields_reflect_null_and_omitted_serialization() {
        let document = crate::openapi_document();
        let required = |name: &str, property: &str| {
            document["components"]["schemas"][name]["required"]
                .as_array()
                .is_some_and(|fields| fields.iter().any(|field| field == property))
        };

        assert!(required("BookSummary", "language"));
        assert!(required("DiscoveryResult", "year"));
        assert!(!required("SpotlightItem", "rating"));
        assert!(!required("ReleaseView", "releaseName"));
        assert!(!required("ReleaseCandidate", "detectedTitle"));
        assert!(!required("ReleaseCandidate", "isCollection"));
        assert!(!required("ProfileUpdate", "preferredFormat"));
        assert!(!required("BookUpdateInput", "description"));
        assert!(!required("BookUpdateInput", "useAutomaticMetadata"));
        assert!(
            document["components"]["schemas"]["BookUpdateInput"]["properties"]["description"]
                .get("default")
                .is_none()
        );
    }
}
