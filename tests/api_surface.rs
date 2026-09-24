//! One API, three descriptions — and this test is what keeps them equal.
//!
//! The service describes itself three times: the routes Rocket mounts in `src/main.rs`,
//! the OpenAPI document in `static/swagger.yaml`, and `static/spec.js`, which feeds the
//! reference, the console and the endpoint count shown on the home page. Nothing forces
//! the three to agree, and a documentation that drifts from the code is not visible: it is
//! discovered by a support ticket, months later.
//!
//! So the comparison runs at `cargo test` time, before anything is deployed and without a
//! running service — it reads the three files and names every endpoint that is on one side
//! and missing from another. A failure that only says "41 ≠ 37" costs an hour; these say
//! which endpoint, and in which file it is missing.
//!
//! # Scope
//!
//! The two documents describe the *integration* surface: everything mounted under `/api`,
//! the MCP entry point, the download URL and the legacy form endpoint. The HTML pages —
//! `/`, `/outils/<slug>`, `/console`, `/sitemap.xml` — are the shop window, read by
//! visitors and crawlers, and belong in no API reference. `in_reference` draws that line in
//! one place; a new route inside the line has to be documented, and that is the point.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Every HTTP method Rocket has an attribute macro for. `route` is deliberately absent:
/// nothing in this service uses it, and a route declared that way would show up as a
/// handler with no verb rather than being silently attributed to GET.
const METHODS: [&str; 7] = ["get", "post", "put", "delete", "patch", "head", "options"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {:?}: {}", path, e))
}

// ------------ The line between the API and the shop window ------------

/// Is this endpoint part of what the reference documents?
fn in_reference(entry: &str) -> bool {
    let path = entry.split_once(' ').map(|(_, path)| path).unwrap_or(entry);
    path.starts_with("/api/")
        || path == "/mcp"
        || path.starts_with("/download/")
        // The original FormData endpoint, kept verbatim for backward compatibility. It is
        // the one integration endpoint that does not live under a prefix of its own.
        || entry == "POST /"
}

// ------------ What the code mounts ------------

/// `("METHOD", "/path")` for every route `src/main.rs` mounts, with Rocket's `<name>`
/// parameters written the way OpenAPI writes them.
fn mounted() -> BTreeSet<String> {
    let handlers = route_attributes();
    let mut inventory = BTreeSet::new();

    for (base, handler) in mounts(&read("src/main.rs")) {
        let (method, path) = handlers.get(&handler).unwrap_or_else(|| {
            panic!(
                "src/main.rs mounts {} but no #[get(…)]/#[post(…)] attribute was found for it",
                handler
            )
        });
        inventory.insert(format!("{} {}", method, join(&base, path)));
    }

    inventory
}

/// `("/api", "routes::convert::convert")` for every entry of every `routes![…]` list.
///
/// Text scanning rather than a Rust parser: the alternative is a syntax crate, and this
/// service ships no dependency it cannot audit by hand.
fn mounts(main_rs: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();

    // Each chunk runs from one `.mount(` to the next, so the base and the route list that
    // belong together stay together, and a mount with no `routes![` (the static file
    // servers) simply contributes nothing.
    for chunk in main_rs.split(".mount(").skip(1) {
        let Some(base) = first_string(chunk) else {
            continue;
        };
        let Some(list) = chunk
            .split_once("routes![")
            .and_then(|(_, rest)| rest.split_once(']').map(|(inside, _)| inside.to_string()))
        else {
            continue;
        };

        for line in list.lines() {
            // Route lists carry comments explaining why an endpoint exists
            let line = line.split("//").next().unwrap_or("");
            for handler in line.split(',') {
                let handler = handler.trim();
                if !handler.is_empty() {
                    found.push((base.clone(), handler.to_string()));
                }
            }
        }
    }

    assert!(
        !found.is_empty(),
        "no routes![…] list found in src/main.rs — this test would pass on an empty service"
    );
    found
}

/// `"routes::files::upload" -> ("POST", "/files")`, read from the attribute macros.
fn route_attributes() -> std::collections::HashMap<String, (String, String)> {
    let mut handlers = std::collections::HashMap::new();
    let dir = root().join("src/routes");

    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot list {:?}: {}", dir, e));
    for entry in entries {
        let path = entry.expect("unreadable directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let module = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("a .rs file has a stem")
            .to_string();
        if module == "mod" {
            continue;
        }

        collect_attributes(&path, &module, &mut handlers);
    }

    handlers
}

fn collect_attributes(
    file: &Path,
    module: &str,
    handlers: &mut std::collections::HashMap<String, (String, String)>,
) {
    let source =
        std::fs::read_to_string(file).unwrap_or_else(|e| panic!("cannot read {:?}: {}", file, e));
    let lines: Vec<&str> = source.lines().collect();

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some((method, rest)) = METHODS.iter().find_map(|method| {
            trimmed
                .strip_prefix(&format!("#[{}(", method))
                .map(|rest| (*method, rest))
        }) else {
            continue;
        };
        let Some(path) = first_string(rest) else {
            continue;
        };

        // The attribute sits directly above its handler; the first declaration below it is
        // the one it decorates.
        let name = lines[index + 1..]
            .iter()
            .find_map(|candidate| function_name(candidate))
            .unwrap_or_else(|| panic!("#[{}(\"{}\")] in {:?} decorates no fn", method, path, file));

        handlers.insert(
            format!("routes::{}::{}", module, name),
            (method.to_uppercase(), path),
        );
    }
}

/// The name declared by a `pub fn` / `pub async fn` line, and nothing else: a doc comment
/// that happens to contain the word "fn" is not a declaration.
fn function_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !(trimmed.starts_with("pub ") || trimmed.starts_with("fn ") || trimmed.starts_with("async "))
    {
        return None;
    }

    let (_, rest) = trimmed.split_once("fn ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The first double-quoted literal of a fragment. Enough for the attributes and mount
/// bases here, none of which contain an escaped quote.
fn first_string(fragment: &str) -> Option<String> {
    let (_, rest) = fragment.split_once('"')?;
    let (inside, _) = rest.split_once('"')?;
    Some(inside.to_string())
}

/// Mount base plus route path, in OpenAPI's notation: `/api` and `/files/<id>` become
/// `/api/files/{id}`, and a query guard (`?<cover>`) is not part of a path at all.
fn join(base: &str, path: &str) -> String {
    let path = path.split('?').next().unwrap_or(path);

    let full = if path == "/" {
        base.to_string()
    } else {
        format!("{}{}", base.trim_end_matches('/'), path)
    };

    let full = if full.is_empty() {
        "/".to_string()
    } else {
        full
    };

    full.split('/')
        .map(
            |segment| match segment.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
                // `<rest..>` is a trailing segment guard; its name is what documents it
                Some(name) => format!("{{{}}}", name.trim_end_matches("..")),
                None => segment.to_string(),
            },
        )
        .collect::<Vec<_>>()
        .join("/")
}

// ------------ What the two documents declare ------------

/// The `paths:` block of the OpenAPI document: two-space indentation names a path,
/// four-space indentation names an operation on it.
fn swagger() -> BTreeSet<String> {
    let source = read("static/swagger.yaml");
    let mut inventory = BTreeSet::new();
    let mut path: Option<String> = None;
    let mut inside = false;

    for line in source.lines() {
        if line.starts_with("paths:") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        // Any other top-level key closes the block
        if !line.is_empty() && !line.starts_with(' ') && !line.starts_with('#') {
            break;
        }

        if let Some(rest) = line.strip_prefix("  ") {
            if rest.starts_with('/') {
                path = rest.strip_suffix(':').map(str::to_string);
                continue;
            }
            if let Some(operation) = rest.strip_prefix("  ") {
                if let Some(method) = METHODS
                    .iter()
                    .find(|method| operation == format!("{}:", method))
                {
                    let path = path
                        .clone()
                        .expect("an operation in swagger.yaml before any path");
                    inventory.insert(format!("{} {}", method.to_uppercase(), path));
                }
            }
        }
    }

    assert!(
        !inventory.is_empty(),
        "no paths read from static/swagger.yaml"
    );
    inventory
}

/// `spec.js` declares one endpoint per object, and its `method` and `path` sit on the same
/// line as its `key`. That is a convention rather than a grammar, so it is stated in the
/// file itself — and splitting them across lines makes this test fail loudly rather than
/// silently drop the endpoint.
fn spec_js() -> BTreeSet<String> {
    let inventory: BTreeSet<String> = spec_js_declarations().into_iter().collect();
    assert!(
        !inventory.is_empty(),
        "no endpoints read from static/spec.js"
    );
    inventory
}

/// Every declaration in order, duplicates included — the sets above cannot see a repeat,
/// and a repeated endpoint is a real failure mode: the console keys its lookup by `key`, so
/// the second copy quietly wins and the reference shows the same route twice.
fn spec_js_declarations() -> Vec<String> {
    read("static/spec.js")
        .lines()
        .filter_map(|line| {
            let method = after_key(line, "method: ")?;
            let path = after_key(line, "path: ")?;
            Some(format!("{} {}", method, path))
        })
        .collect()
}

/// The string value of `key: "value"` on a line, ignoring a `buildPath:` that merely ends
/// in the same letters.
fn after_key(line: &str, key: &str) -> Option<String> {
    let mut from = 0;
    while let Some(offset) = line[from..].find(key) {
        let start = from + offset;
        let preceded_by_a_name = line[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !preceded_by_a_name {
            return first_string(&line[start + key.len()..]);
        }
        from = start + key.len();
    }
    None
}

// ------------ The comparison ------------

/// A difference, worded so the fix is obvious without opening anything.
fn report(missing: &BTreeSet<String>, from: &str, present_in: &str) -> String {
    format!(
        "{} endpoint(s) missing from {} but {}:\n  {}",
        missing.len(),
        from,
        present_in,
        missing.iter().cloned().collect::<Vec<_>>().join("\n  ")
    )
}

fn agree(left: (&str, &BTreeSet<String>), right: (&str, &BTreeSet<String>)) {
    let (left_name, left_set) = left;
    let (right_name, right_set) = right;

    let missing_right: BTreeSet<String> = left_set.difference(right_set).cloned().collect();
    let missing_left: BTreeSet<String> = right_set.difference(left_set).cloned().collect();

    let mut complaints = Vec::new();
    if !missing_right.is_empty() {
        complaints.push(report(
            &missing_right,
            right_name,
            &format!("declared by {}", left_name),
        ));
    }
    if !missing_left.is_empty() {
        complaints.push(report(
            &missing_left,
            left_name,
            &format!("declared by {}", right_name),
        ));
    }

    assert!(
        complaints.is_empty(),
        "{} and {} describe different services.\n\n{}\n\nDescribe the endpoint in both \
         documents, or delete it from the one that still claims it.",
        left_name,
        right_name,
        complaints.join("\n\n")
    );
}

fn reference_surface() -> BTreeSet<String> {
    mounted()
        .into_iter()
        .filter(|entry| in_reference(entry))
        .collect()
}

#[test]
fn swagger_describes_exactly_the_routes_the_service_mounts() {
    agree(
        ("src/main.rs", &reference_surface()),
        ("static/swagger.yaml", &swagger()),
    );
}

#[test]
fn spec_js_describes_exactly_the_routes_the_service_mounts() {
    agree(
        ("src/main.rs", &reference_surface()),
        ("static/spec.js", &spec_js()),
    );
}

/// Redundant while the two tests above pass, and the first one to fail in a useful way
/// when someone updates one document and forgets the other: the console shows the count
/// from `spec.js`, so this is the pair a reader is most likely to see disagree.
#[test]
fn the_two_documents_agree_with_each_other() {
    agree(
        ("static/swagger.yaml", &swagger()),
        ("static/spec.js", &spec_js()),
    );
}

/// Two people documenting the same new endpoint in the same file is not hypothetical: it
/// happened while this test was being written, and the set comparison above cannot see it.
#[test]
fn no_endpoint_is_declared_twice_in_spec_js() {
    let mut seen = BTreeSet::new();
    let repeated: BTreeSet<String> = spec_js_declarations()
        .into_iter()
        .filter(|endpoint| !seen.insert(endpoint.clone()))
        .collect();

    assert!(
        repeated.is_empty(),
        "static/spec.js declares the same endpoint more than once. The console keys its \
         lookup by `key`, so the second copy silently wins and the reference lists the route \
         twice:\n  {}",
        repeated.iter().cloned().collect::<Vec<_>>().join("\n  ")
    );
}

/// The HTML pages are outside the reference on purpose, not by accident. If the site ever
/// loses every page, the exclusion has stopped meaning anything and the line drawn by
/// `in_reference` needs redrawing rather than trusting.
#[test]
fn the_shop_window_is_what_is_left_outside_the_reference() {
    let outside: BTreeSet<String> = mounted()
        .into_iter()
        .filter(|entry| !in_reference(entry))
        .collect();

    for expected in ["GET /", "GET /console", "GET /sitemap.xml"] {
        assert!(
            outside.contains(expected),
            "{} is no longer mounted, or no longer outside the API reference. \
             Outside the reference today: {:?}",
            expected,
            outside
        );
    }
}

// ------------ The parsing itself ------------

#[cfg(test)]
mod parsing {
    use super::*;

    #[test]
    fn a_rocket_parameter_becomes_an_openapi_one() {
        assert_eq!(join("/api", "/files/<id>/meta"), "/api/files/{id}/meta");
        assert_eq!(
            join("/download", "/<client_id>/<pdf_name>"),
            "/download/{client_id}/{pdf_name}"
        );
    }

    /// A query guard is not part of the path, and OpenAPI describes it as a parameter
    #[test]
    fn a_query_guard_is_not_part_of_the_path() {
        assert_eq!(
            join("/api", "/themes/<name>/<version>/preview.png?<cover>"),
            "/api/themes/{name}/{version}/preview.png"
        );
    }

    /// The two roots that are easy to turn into `//`
    #[test]
    fn a_route_at_the_root_of_its_mount_keeps_the_base() {
        assert_eq!(join("/", "/"), "/");
        assert_eq!(join("/mcp", "/"), "/mcp");
    }

    #[test]
    fn the_line_between_the_api_and_the_site_is_drawn_where_it_is_meant_to_be() {
        assert!(in_reference("POST /api/convert"));
        assert!(in_reference("GET /mcp"));
        assert!(in_reference("GET /download/{client_id}/{pdf_name}"));
        assert!(in_reference("POST /"));

        assert!(!in_reference("GET /"));
        assert!(!in_reference("GET /outils/{slug}"));
        assert!(!in_reference("GET /console"));
    }

    #[test]
    fn a_mount_without_a_route_list_contributes_nothing() {
        let source = r#"
            .mount("/static", FileServer::from("static"))
            .mount("/api", routes![routes::health::health, routes::convert::convert])
        "#;
        assert_eq!(
            mounts(source),
            vec![
                ("/api".to_string(), "routes::health::health".to_string()),
                ("/api".to_string(), "routes::convert::convert".to_string()),
            ]
        );
    }

    #[test]
    fn a_comment_in_a_route_list_is_not_a_handler() {
        let source = "
            .mount(\"/api\", routes![
                // Ingestion: the way a caller's own file gets in
                routes::files::upload,
            ])
        ";
        assert_eq!(
            mounts(source),
            vec![("/api".to_string(), "routes::files::upload".to_string())]
        );
    }

    /// `buildPath: (v) => …` must not be read as a path declaration
    #[test]
    fn build_path_is_not_a_path() {
        assert_eq!(after_key("buildPath: (v) => \"/api/x\"", "path: "), None);
        assert_eq!(
            after_key(
                "key: \"health\", method: \"GET\", path: \"/api/health\"",
                "path: "
            ),
            Some("/api/health".to_string())
        );
    }
}
