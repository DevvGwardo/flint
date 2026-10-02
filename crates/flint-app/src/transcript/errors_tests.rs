use super::ErrorKind;
use super::classify;

#[test]
fn classifies_failures() {
    assert_eq!(
        classify("No API key found. Set FLINT_API_KEY…"),
        ErrorKind::NoKey
    );
    assert_eq!(
        classify(
            "request failed: error sending request for url (http://127.0.0.1:9/v1): Connection refused"
        ),
        ErrorKind::Unreachable
    );
    assert_eq!(
        classify("This model endpoint doesn't accept reasoning_effort; continuing without it."),
        ErrorKind::EffortUnsupported
    );
    assert_eq!(classify("HTTP 400: model not found"), ErrorKind::Model);
}
