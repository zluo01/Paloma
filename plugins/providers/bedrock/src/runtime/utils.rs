use std::error::Error;

use aws_smithy_types::error::metadata::ProvideErrorMetadata;

pub(super) fn parse_error<E>(error: &E) -> String
where
    E: ProvideErrorMetadata + Error,
{
    if let Some(message) = error.message() {
        return match error.code() {
            Some(code) => format!("{code}: {message}"),
            None => message.to_string(),
        };
    }

    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}
