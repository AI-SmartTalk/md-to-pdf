use rocket::fs::NamedFile;
use rocket::response::Redirect;

fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 80
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
}

fn landing_file(language: &str, page: &str) -> Option<&'static str> {
    match (language, page) {
        ("fr", "convertir-markdown-pdf") => Some("static/seo/markdown-to-pdf.html"),
        ("fr", "api-generation-pdf") => Some("static/seo/markdown-to-pdf-api.html"),
        ("en", "markdown-to-pdf") => Some("static/seo/en/markdown-to-pdf.html"),
        ("en", "pdf-generation-api") => Some("static/seo/en/pdf-generation-api.html"),
        ("es", "convertidor-markdown-pdf") => Some("static/seo/es/convertidor-markdown-pdf.html"),
        ("es", "api-generacion-pdf") => Some("static/seo/es/api-generacion-pdf.html"),
        ("de", "markdown-zu-pdf") => Some("static/seo/de/markdown-zu-pdf.html"),
        ("de", "pdf-generierungs-api") => Some("static/seo/de/pdf-generierungs-api.html"),
        ("it", "markdown-in-pdf") => Some("static/seo/it/markdown-in-pdf.html"),
        ("it", "api-generazione-pdf") => Some("static/seo/it/api-generazione-pdf.html"),
        ("pt", "conversor-markdown-para-pdf") => {
            Some("static/seo/pt/conversor-markdown-para-pdf.html")
        }
        ("pt", "api-geracao-pdf") => Some("static/seo/pt/api-geracao-pdf.html"),
        _ => None,
    }
}

async fn product_landing_for(language: &str, page: &str) -> Option<NamedFile> {
    if !valid_slug(page) {
        return None;
    }
    NamedFile::open(landing_file(language, page)?).await.ok()
}

async fn blog_index_for(language: &str) -> Option<NamedFile> {
    NamedFile::open(format!("static/blog-generated/{language}/index.html"))
        .await
        .ok()
}

async fn article_for(language: &str, slug: &str) -> Option<NamedFile> {
    if !valid_slug(slug) {
        return None;
    }
    NamedFile::open(format!("static/blog-generated/{language}/{slug}.html"))
        .await
        .ok()
}

#[get("/<page>")]
pub async fn fr_product_landing(page: &str) -> Option<NamedFile> {
    product_landing_for("fr", page).await
}

#[get("/blog")]
pub async fn fr_blog_index() -> Option<NamedFile> {
    blog_index_for("fr").await
}

#[get("/blog/<slug>")]
pub async fn fr_article(slug: &str) -> Option<NamedFile> {
    article_for("fr", slug).await
}

#[get("/<page>")]
pub async fn en_product_landing(page: &str) -> Option<NamedFile> {
    product_landing_for("en", page).await
}

#[get("/blog")]
pub async fn en_blog_index() -> Option<NamedFile> {
    blog_index_for("en").await
}

#[get("/blog/<slug>")]
pub async fn en_article(slug: &str) -> Option<NamedFile> {
    article_for("en", slug).await
}

#[get("/<page>")]
pub async fn es_product_landing(page: &str) -> Option<NamedFile> {
    product_landing_for("es", page).await
}

#[get("/blog")]
pub async fn es_blog_index() -> Option<NamedFile> {
    blog_index_for("es").await
}

#[get("/blog/<slug>")]
pub async fn es_article(slug: &str) -> Option<NamedFile> {
    article_for("es", slug).await
}

#[get("/<page>")]
pub async fn de_product_landing(page: &str) -> Option<NamedFile> {
    product_landing_for("de", page).await
}

#[get("/blog")]
pub async fn de_blog_index() -> Option<NamedFile> {
    blog_index_for("de").await
}

#[get("/blog/<slug>")]
pub async fn de_article(slug: &str) -> Option<NamedFile> {
    article_for("de", slug).await
}

#[get("/<page>")]
pub async fn it_product_landing(page: &str) -> Option<NamedFile> {
    product_landing_for("it", page).await
}

#[get("/blog")]
pub async fn it_blog_index() -> Option<NamedFile> {
    blog_index_for("it").await
}

#[get("/blog/<slug>")]
pub async fn it_article(slug: &str) -> Option<NamedFile> {
    article_for("it", slug).await
}

#[get("/<page>")]
pub async fn pt_product_landing(page: &str) -> Option<NamedFile> {
    product_landing_for("pt", page).await
}

#[get("/blog")]
pub async fn pt_blog_index() -> Option<NamedFile> {
    blog_index_for("pt").await
}

#[get("/blog/<slug>")]
pub async fn pt_article(slug: &str) -> Option<NamedFile> {
    article_for("pt", slug).await
}

#[get("/blog")]
pub fn old_blog_index() -> Redirect {
    Redirect::permanent("/fr/blog")
}

#[get("/blog/<slug>")]
pub fn old_blog_article(slug: &str) -> Option<Redirect> {
    valid_slug(slug).then(|| Redirect::permanent(format!("/fr/blog/{slug}")))
}

#[get("/markdown-to-pdf")]
pub fn old_converter() -> Redirect {
    Redirect::permanent("/fr/convertir-markdown-pdf")
}

#[get("/markdown-to-pdf-api")]
pub fn old_api_page() -> Redirect {
    Redirect::permanent("/fr/api-generation-pdf")
}

#[cfg(test)]
mod tests {
    use super::landing_file;

    #[test]
    fn all_localized_public_landing_routes_are_whitelisted() {
        let routes = [
            ("fr", "convertir-markdown-pdf"),
            ("fr", "api-generation-pdf"),
            ("en", "markdown-to-pdf"),
            ("en", "pdf-generation-api"),
            ("es", "convertidor-markdown-pdf"),
            ("es", "api-generacion-pdf"),
            ("de", "markdown-zu-pdf"),
            ("de", "pdf-generierungs-api"),
            ("it", "markdown-in-pdf"),
            ("it", "api-generazione-pdf"),
            ("pt", "conversor-markdown-para-pdf"),
            ("pt", "api-geracao-pdf"),
        ];
        for (language, page) in routes {
            assert!(landing_file(language, page).is_some(), "{language}/{page}");
        }
        assert!(landing_file("es", "..%2fsecrets").is_none());
        assert!(landing_file("ru", "markdown-to-pdf").is_none());
    }
}
