use iris_ui::geom::Bounds;
use iris_ui::view::Align::Center;
use iris_ui::view::Flex::{Grow, Shrink};
use iris_ui::view::{View, ViewId};

pub fn make_h_spacer(id: &ViewId) -> View {
    View {
        name: id.clone(),
        h_flex: Grow,
        h_align: Center,
        v_flex: Shrink,
        v_align: Center,
        bounds: Bounds::new(0, 0, 10, 10),
        visible: true,
        title: "spacer".into(),
        draw: None,
        input: None,
        layout: Some(|e| {
            if let Some(view) = e.scene.get_view_mut(&e.target) {
                if view.h_flex == Grow {
                    view.bounds.size.w = e.space.w;
                }
                if view.v_flex == Grow {
                    view.bounds.size.h = e.space.h;
                }
            }
        }),
        state: None,
    }
}
