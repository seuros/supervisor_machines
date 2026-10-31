use super::{DEFAULT_RESTARTS, DEFAULT_WINDOW_NS, Intensity, Span};

#[test]
fn a_span_always_covers_at_least_the_child_that_died() {
    let single = Span::single(4);

    assert_eq!(single.len(), 1);
    assert!(!single.is_empty());
    assert!(single.contains(4));
    assert!(!single.contains(3));
}

#[test]
fn a_wide_span_counts_both_ends() {
    let span = Span { first: 2, last: 5 };

    assert_eq!(span.len(), 4);
    assert!(span.contains(2) && span.contains(5));
    assert!(!span.contains(6));
}

#[test]
fn the_default_intensity_is_otps() {
    let intensity = Intensity::default();

    assert_eq!(intensity.restarts, DEFAULT_RESTARTS);
    assert_eq!(intensity.window_ns, DEFAULT_WINDOW_NS);
}
