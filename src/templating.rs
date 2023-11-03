use askama::Template;
use axum::{response::{IntoResponse, Html}, http::StatusCode};

pub struct HtmlTemplate<T>(pub T);

impl<T> IntoResponse for HtmlTemplate<T>
where
    T: Template
{
    fn into_response(self) -> axum::response::Response {
        match self.0.render() {
            Ok(html) => Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to render template: {}", err),
            ).into_response()
        }
    }
}

#[derive(Template)]
#[template(path = "pages/hello.html")]
pub struct ExamplePage;
