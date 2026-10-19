//! Native IME DOM adoption must retain live text nodes and browser selection.
#![cfg(target_arch = "wasm32")]

use taino_edit_core::{DomSpec, MarkSpec, Node, NodeSpec, Schema, SchemaBuilder, Selection};
use taino_edit_dom::{EditorView, ViewDesc};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{Element, Text};

wasm_bindgen_test_configure!(run_in_browser);

fn schema() -> Schema {
    SchemaBuilder::new()
        .node(
            "doc",
            NodeSpec {
                content: Some("block+".into()),
                ..Default::default()
            },
        )
        .node(
            "paragraph",
            NodeSpec {
                content: Some("inline*".into()),
                group: Some("block".into()),
                to_dom: Some(|_| DomSpec::element("p")),
                ..Default::default()
            },
        )
        .node(
            "text",
            NodeSpec {
                group: Some("inline".into()),
                ..Default::default()
            },
        )
        .mark(
            "strong",
            MarkSpec {
                to_dom: Some(|_| DomSpec::element("strong")),
                ..Default::default()
            },
        )
        .mark(
            "em",
            MarkSpec {
                to_dom: Some(|_| DomSpec::element("em")),
                ..Default::default()
            },
        )
        .top_node("doc")
        .build()
        .unwrap()
}

fn doc(s: &Schema, text: &str, marks: &[&str]) -> Node {
    let marks = marks
        .iter()
        .map(|name| s.mark_type(name).unwrap().create(Default::default()))
        .collect();
    let children = if text.is_empty() {
        vec![]
    } else {
        vec![s.text(text, marks).unwrap()]
    };
    let paragraph = s
        .node("paragraph", Default::default(), children, vec![])
        .unwrap();
    s.node("doc", Default::default(), vec![paragraph], vec![])
        .unwrap()
}

fn attach(doc: Node, s: Schema) -> (EditorView, Element) {
    let document = web_sys::window().unwrap().document().unwrap();
    let root = document.create_element("div").unwrap();
    document.body().unwrap().append_child(&root).unwrap();
    let view = EditorView::mount(doc, s, root.clone());
    (view, root)
}

fn cleanup(root: Element) {
    root.parent_node().unwrap().remove_child(&root).unwrap();
}

#[wasm_bindgen_test]
fn native_text_and_utf16_selection_are_adopted_without_replacement() {
    let s = schema();
    let (mut view, root) = attach(doc(&s, "가", &[]), s.clone());
    let paragraph = root.first_child().unwrap();
    let text: Text = paragraph.first_child().unwrap().dyn_into().unwrap();
    text.set_data("가나😀");
    let selection = web_sys::window().unwrap().get_selection().unwrap().unwrap();
    selection
        .collapse_with_offset(Some(text.as_ref()), 4)
        .unwrap();
    let html = root.inner_html();
    let expected = doc(&s, "가나😀", &[]);

    assert!(view.adopt_dom(expected.clone()));
    assert_eq!(view.doc(), &expected);
    assert_eq!(root.inner_html(), html);
    assert!(root.first_child().unwrap().is_same_node(Some(&paragraph)));
    assert!(selection
        .anchor_node()
        .unwrap()
        .is_same_node(Some(text.as_ref())));
    assert_eq!(selection.anchor_offset(), 4);
    assert_eq!(view.read_selection(), Some(Selection::caret(4)));
    assert!(view.read_dom_changes().is_none());
    cleanup(root);
}

#[wasm_bindgen_test]
fn native_multiple_mark_wrappers_are_bound_to_existing_nodes() {
    let s = schema();
    let (mut view, root) = attach(doc(&s, "", &[]), s.clone());
    root.set_inner_html("<p><em><strong>한😀</strong></em></p>");
    let wrapper = root.first_child().unwrap().first_child().unwrap();
    let text: Text = wrapper
        .first_child()
        .unwrap()
        .first_child()
        .unwrap()
        .dyn_into()
        .unwrap();
    let selection = web_sys::window().unwrap().get_selection().unwrap().unwrap();
    selection
        .collapse_with_offset(Some(text.as_ref()), 3)
        .unwrap();
    let html = root.inner_html();

    assert!(view.adopt_dom(doc(&s, "한😀", &["strong", "em"])));
    let ViewDesc::Element { children, .. } = &view.children()[0] else {
        panic!("paragraph")
    };
    let ViewDesc::Text {
        text: bound,
        wrapper: Some(bound_wrapper),
        ..
    } = &children[0]
    else {
        panic!("marked text")
    };
    assert!(bound.is_same_node(Some(text.as_ref())));
    assert!(bound_wrapper.is_same_node(Some(&wrapper)));
    assert_eq!(root.inner_html(), html);
    assert!(selection
        .anchor_node()
        .unwrap()
        .is_same_node(Some(text.as_ref())));
    assert_eq!(selection.anchor_offset(), 3);
    assert_eq!(view.read_selection(), Some(Selection::caret(3)));
    cleanup(root);
}

#[wasm_bindgen_test]
fn presentation_breaks_are_accepted_without_removal() {
    let s = schema();
    for (html, text) in [
        ("<p><br data-taino-trailing-break=\"\"></p>", ""),
        ("<p><br></p>", ""),
        ("<p></p>", ""),
        ("<p>한<br data-taino-trailing-break=\"\"></p>", "한"),
        ("<p>한<br></p>", "한"),
    ] {
        let (mut view, root) = attach(doc(&s, "old", &[]), s.clone());
        root.set_inner_html(html);
        let html = root.inner_html();
        let paragraph = root.first_child().unwrap();
        let last = paragraph.last_child();
        assert!(view.adopt_dom(doc(&s, text, &[])), "{html}");
        assert_eq!(root.inner_html(), html);
        assert!(root.first_child().unwrap().is_same_node(Some(&paragraph)));
        if let Some(last) = last {
            assert!(paragraph.last_child().unwrap().is_same_node(Some(&last)));
        }
        cleanup(root);
    }
}

#[wasm_bindgen_test]
fn mismatches_leave_the_document_descriptors_and_dom_unchanged() {
    let s = schema();
    for html in [
        "<p>가<br>나</p>",
        "<p>가나<br><br></p>",
        "<p class=\"unexpected\">가나</p>",
        "<p><span>가나</span></p>",
        "<p>다른 글</p>",
        "<p>가나<br title=\"not-a-placeholder\"></p>",
    ] {
        let original = doc(&s, "old", &[]);
        let (mut view, root) = attach(original.clone(), s.clone());
        let old_desc_node = view.children()[0].dom_node();
        root.set_inner_html(html);
        let html = root.inner_html();
        let live_node = root.first_child().unwrap();
        assert!(!view.adopt_dom(doc(&s, "가나", &[])), "{html}");
        assert_eq!(view.doc(), &original);
        assert!(view.children()[0]
            .dom_node()
            .is_same_node(Some(&old_desc_node)));
        assert_eq!(root.inner_html(), html);
        assert!(root.first_child().unwrap().is_same_node(Some(&live_node)));
        cleanup(root);
    }
}

#[wasm_bindgen_test]
fn adjacent_native_text_nodes_are_not_silently_merged() {
    let s = schema();
    let original = doc(&s, "가", &[]);
    let (mut view, root) = attach(original.clone(), s.clone());
    let paragraph = root.first_child().unwrap();
    let first = paragraph.first_child().unwrap();
    let second = root.owner_document().unwrap().create_text_node("나");
    paragraph.append_child(&second).unwrap();
    assert!(!view.adopt_dom(doc(&s, "가나", &[])));
    assert_eq!(view.doc(), &original);
    assert!(paragraph.first_child().unwrap().is_same_node(Some(&first)));
    assert!(paragraph
        .last_child()
        .unwrap()
        .is_same_node(Some(second.as_ref())));
    cleanup(root);
}

#[wasm_bindgen_test]
fn adopted_bare_break_is_removed_before_appending_a_marked_inline_sibling() {
    let s = schema();
    let (mut view, root) = attach(doc(&s, "", &[]), s.clone());
    root.set_inner_html("<p>ab<br></p>");
    let paragraph = root.first_child().unwrap();
    let first = paragraph.first_child().unwrap();
    let placeholder = paragraph.last_child().unwrap();
    assert!(view.adopt_dom(doc(&s, "ab", &[])));
    assert!(placeholder.parent_node().is_some()); // adoption itself remains read-only

    let bold = s.mark_type("strong").unwrap().create(Default::default());
    let new_paragraph = s
        .node(
            "paragraph",
            Default::default(),
            vec![
                s.text("a", vec![]).unwrap(),
                s.text("b", vec![bold]).unwrap(),
            ],
            vec![],
        )
        .unwrap();
    let new_doc = s
        .node("doc", Default::default(), vec![new_paragraph], vec![])
        .unwrap();
    view.update(new_doc);

    assert_eq!(root.inner_html(), "<p>a<strong>b</strong></p>");
    assert!(root.first_child().unwrap().is_same_node(Some(&paragraph)));
    assert!(paragraph.first_child().unwrap().is_same_node(Some(&first)));
    assert!(placeholder.parent_node().is_none());
    cleanup(root);
}

#[wasm_bindgen_test]
fn update_does_not_remove_a_different_or_modified_break() {
    let s = schema();
    for replace in [false, true] {
        let expected = doc(&s, "ab", &[]);
        let (mut view, root) = attach(expected.clone(), s.clone());
        root.set_inner_html("<p>ab<br></p>");
        let paragraph = root.first_child().unwrap();
        let adopted: Element = paragraph.last_child().unwrap().dyn_into().unwrap();
        assert!(view.adopt_dom(expected.clone()));
        let unrelated = if replace {
            let new = root.owner_document().unwrap().create_element("br").unwrap();
            paragraph.replace_child(&new, &adopted).unwrap();
            new
        } else {
            adopted.set_attribute("title", "user-owned").unwrap();
            adopted
        };
        let html = root.inner_html();
        view.update(expected);
        assert_eq!(root.inner_html(), html);
        assert!(paragraph
            .last_child()
            .unwrap()
            .is_same_node(Some(unrelated.as_ref())));
        cleanup(root);
    }
}
