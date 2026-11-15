//! The docs built into the binary all draw, diagrams included.

use the_deaner::cheatsheet::guide_pages;
use the_deaner_ui::markdown::render;

#[test]
fn every_page_draws_at_any_width() {
    for page in guide_pages() {
        for width in [40, 80, 120, 200] {
            let r = render(page.source, width);
            assert!(r.lines.len() > 5, "{} at {width}", page.slug);
        }
    }
}

#[test]
fn the_docs_diagrams_are_drawn_at_a_normal_popup_width() {
    for page in guide_pages() {
        let text = render(page.source, 140).plain().join("\n");
        assert!(
            !text.contains("its mermaid source"),
            "{} shows a diagram as source at 140 columns",
            page.slug
        );
    }
}
