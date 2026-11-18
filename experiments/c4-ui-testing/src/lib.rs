slint::slint! {
    import { Button } from "std-widgets.slint";
    export component Demo inherits Window {
        in-out property <string> label: "before";
        VerticalLayout {
            go := Button { text: "Go"; clicked => { root.label = "after"; } }
            status := Text { text: root.label; accessible-role: text; accessible-label: "status"; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use i_slint_backend_testing::ElementHandle;

    #[test]
    fn clicking_the_button_changes_the_text() {
        i_slint_backend_testing::init_no_event_loop();
        let demo = Demo::new().unwrap();
        let button = ElementHandle::find_by_element_id(&demo, "Demo::go").next().expect("button");
        button.invoke_accessible_default_action();
        assert_eq!(demo.get_label(), "after");
        let status = ElementHandle::find_by_element_id(&demo, "Demo::status").next().expect("status text");
        assert_eq!(status.accessible_label().unwrap_or_default(), "status");
    }
}
