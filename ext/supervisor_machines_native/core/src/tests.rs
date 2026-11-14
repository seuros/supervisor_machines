use core::time::Duration;

use super::{DEFAULT_RESTARTS, DEFAULT_WINDOW, Intensity, Refusal, Span};

/// The value inside, or a test failure naming what was missing.
pub fn some<T>(value: Option<T>) -> T {
    value.expect("expected a value, got None")
}

/// A span covers both ends and always at least the child that died; one
/// ending before it starts does not exist.
#[test]
fn a_span_covers_both_ends_and_never_nothing() {
    let single = Span::single(4);
    assert_eq!(Span::new(4, 4), Some(single));
    assert_eq!((single.len(), single.is_empty()), (1, false));
    assert!(single.contains(4) && !single.contains(3));

    let wide = some(Span::new(2, 5));
    assert_eq!((wide.first(), wide.last(), wide.len()), (2, 5, 4));
    assert!(wide.contains(2) && wide.contains(5) && !wide.contains(6));
    assert_eq!(some(Span::new(0, usize::MAX)).len(), usize::MAX);

    assert_eq!(Span::new(5, 2), None);
}

#[test]
fn an_intensity_needs_a_window_and_defaults_to_otps() {
    let intensity = Intensity::default();
    assert_eq!(
        (intensity.restarts(), intensity.window()),
        (DEFAULT_RESTARTS, DEFAULT_WINDOW)
    );
    assert_eq!(
        Intensity::new(DEFAULT_RESTARTS, DEFAULT_WINDOW),
        Some(intensity)
    );

    assert_eq!(Intensity::new(3, Duration::ZERO), None);
    assert!(Intensity::new(0, Duration::from_nanos(1)).is_some());
}

#[test]
fn a_refusal_is_an_error_that_says_why() {
    extern crate std;
    use std::string::ToString;

    let refusal: &dyn core::error::Error = &Refusal::UnknownChild;
    assert!(refusal.to_string().contains("no child"));
}
