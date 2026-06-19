//! Logical output layout: where each connected output sits in the one shared
//! desktop coordinate space.
//!
//! Without a layout every output mirrors the whole scene from the origin, so two
//! monitors show the same pixels. A real layout instead gives every output its
//! own region of one coordinate space, so a window lives at a single position
//! across the whole desktop and each output paints only the part it covers.
//!
//! The policy is the common default: outputs laid left to right in the order
//! given, each at the top (`y = 0`), the next starting where the previous one
//! ended. It is a pure function of the output sizes, so it is unit-tested with no
//! display, the same split the rest of the compositor uses; the DRM backend feeds
//! it the real connected modes and maps each output into the `Space` at the
//! position it returns. Plain integers, no Wayland types, so this builds and
//! tests on every host, not only Linux.

/// Logical positions for `sizes`, laid left to right and top-aligned. Each entry
/// is a `(width, height)` in logical pixels, in the order the outputs should
/// appear; the returned `(x, y)` at index `i` is where output `i` goes. The first
/// sits at the origin and each next one starts at the right edge of those before
/// it. A non-positive width adds no advance (it stacks at the current `x`), so a
/// bogus mode cannot drag later outputs off into negative space.
pub fn arrange(sizes: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut positions = Vec::with_capacity(sizes.len());
    let mut x = 0;
    for &(w, _) in sizes {
        positions.push((x, 0));
        x += w.max(0);
    }
    positions
}

/// The bounding size `(width, height)` of the whole desktop the [`arrange`]d
/// outputs span: the sum of the widths and the tallest height. This is the box a
/// cursor roams over a multi-monitor desktop, so the pointer can cross from one
/// screen to the next instead of being trapped on the first. Empty input spans
/// nothing.
pub fn span(sizes: &[(i32, i32)]) -> (i32, i32) {
    let width = sizes.iter().map(|&(w, _)| w.max(0)).sum();
    let height = sizes.iter().map(|&(_, h)| h.max(0)).max().unwrap_or(0);
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_spans_nothing() {
        assert_eq!(arrange(&[]), Vec::<(i32, i32)>::new());
        assert_eq!(span(&[]), (0, 0));
    }

    #[test]
    fn single_output_sits_at_the_origin() {
        assert_eq!(arrange(&[(1920, 1080)]), vec![(0, 0)]);
        assert_eq!(span(&[(1920, 1080)]), (1920, 1080));
    }

    #[test]
    fn outputs_stack_left_to_right() {
        let sizes = [(1920, 1080), (1280, 1024), (800, 600)];
        assert_eq!(arrange(&sizes), vec![(0, 0), (1920, 0), (3200, 0)]);
        // Width is the sum, height is the tallest.
        assert_eq!(span(&sizes), (4000, 1080));
    }

    #[test]
    fn differing_heights_stay_top_aligned() {
        // Every output is pinned to y = 0 regardless of height.
        let sizes = [(800, 600), (800, 1200)];
        assert_eq!(arrange(&sizes), vec![(0, 0), (800, 0)]);
        assert_eq!(span(&sizes), (1600, 1200));
    }

    #[test]
    fn a_bogus_width_does_not_advance_later_outputs() {
        // A zero or negative width contributes no horizontal advance, so the
        // next real output still lands at a sane position rather than overlapping
        // off to the left.
        let sizes = [(0, 0), (1024, 768)];
        assert_eq!(arrange(&sizes), vec![(0, 0), (0, 0)]);
        assert_eq!(span(&sizes), (1024, 768));
    }
}
