//! Declarative property bindings on top of the canonical state pattern.
//!
//! Application state lives in `Property` cells. Widget signals write into
//! those cells, property subscriptions derive validity, and
//! `UiRuntime::bind` copies validity into `Button::enabled` and a quantity into
//! `NumberInput::value`. Watched and bound properties wake the runtime
//! themselves, so the event loop needs no dirty flag: `take_redraw_request`
//! already reports every property change.
//!
//! Run it with `cargo run -p drawen --example bound_app`.

use std::rc::Rc;

use drawen::{
    Binding, Button, Checkbox, ChildLayout, Clipboard, Color, Constraints, LayoutContext,
    LayoutResult, NumberInput, Point, Property, Rect, Size, Subscription, Text, TextField,
    UiEnvironment, UiRuntime, Widget,
    platform::{WindowAction, WindowApplication, WindowEvent, WindowOptions, WindowPresentation},
    render::{DisplayList, FontdueTextShaper, PaintContext, TextStyle},
};

fn main() -> Result<(), drawen_platform_winit::RunError> {
    drawen_platform_winit::run(BoundApp::new())
}

/// Plain application state expressed as addressable properties.
struct FormState {
    name: Property<String>,
    agreed: Property<bool>,
    quantity: Property<f64>,
    is_valid: Property<bool>,
    submissions: Property<u32>,
}

impl FormState {
    fn new() -> Self {
        Self {
            name: Property::new(String::new()),
            agreed: Property::new(false),
            quantity: Property::new(1.0),
            is_valid: Property::new(false),
            submissions: Property::new(0),
        }
    }

    fn revalidate(&self) {
        self.is_valid
            .set(!self.name.get().trim().is_empty() && self.agreed.get());
    }
}

/// A presentation widget that reads shared properties while painting.
struct FormView {
    state: Rc<FormState>,
}

impl Widget for FormView {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let size = available_size(constraints);
        let field_width = (size.width() - 48.0).max(0.0);
        let mut children = Vec::new();
        let mut y = 96.0;
        for index in 0..context.child_count() {
            let width = if index == 0 {
                field_width
            } else {
                size.width()
            };
            let (id, layout) = context
                .layout_child(
                    index,
                    Constraints::loose(Size::new(width, size.height()).expect("valid child size")),
                )
                .expect("form view owns its children");
            children.push(ChildLayout::new(id, Point::new(24.0, y)));
            y += layout.size().height() + 16.0;
        }
        LayoutResult::with_children(size, children)
    }

    fn paint(&self, context: &mut PaintContext<'_>, _: &UiEnvironment, bounds: Rect) {
        context.fill_rect(bounds, Color::WHITE);
        let shaper = FontdueTextShaper::ubuntu_light();
        let muted = Color::rgba8(90, 100, 110, 255);

        let title = shaper.layout("Drawen bound form", &TextStyle::new(14.0, muted), None);
        context.draw_text(
            title,
            Point::new(bounds.origin.x + 24.0, bounds.origin.y + 24.0),
        );

        let count = format!("Submissions: {}", self.state.submissions.get());
        let count = shaper.layout(&count, &TextStyle::new(24.0, Color::BLACK), None);
        context.draw_text(
            count,
            Point::new(bounds.origin.x + 24.0, bounds.origin.y + 48.0),
        );

        let quantity = format!("Quantity: {}", self.state.quantity.get());
        let quantity = shaper.layout(&quantity, &TextStyle::new(12.0, muted), None);
        context.draw_text(
            quantity,
            Point::new(bounds.origin.x + 24.0, bounds.origin.y + 76.0),
        );

        let hint = if self.state.is_valid.get() {
            "Ready to submit."
        } else {
            "Enter a name and agree to enable Submit."
        };
        let hint = shaper.layout(hint, &TextStyle::new(12.0, muted), None);
        context.draw_text(
            hint,
            Point::new(bounds.origin.x + 24.0, bounds.origin.y + 284.0),
        );
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

struct BoundApp {
    runtime: UiRuntime,
    size: Size,
    _bindings: Vec<Binding>,
    _subscriptions: Vec<Subscription>,
}

impl BoundApp {
    fn new() -> Self {
        let state = Rc::new(FormState::new());
        let mut runtime = UiRuntime::new(FormView {
            state: Rc::clone(&state),
        });

        // Widget signals write into state properties.
        let field = TextField::new();
        let name_subscription = field.changed().subscribe({
            let state = Rc::clone(&state);
            move |text| {
                state.name.set(text.clone());
            }
        });
        runtime
            .append_child(runtime.root(), field)
            .expect("root exists");

        let checkbox = Checkbox::new();
        let agreed_subscription = checkbox.toggled().subscribe({
            let state = Rc::clone(&state);
            move |checked| {
                state.agreed.set(*checked);
            }
        });
        let checkbox = runtime
            .append_child(runtime.root(), checkbox)
            .expect("root exists");
        runtime
            .append_child(
                checkbox,
                Text::styled("I agree to the terms", TextStyle::new(14.0, Color::BLACK)),
            )
            .expect("checkbox exists");

        // A numeric field bound to state: the binding copies state into the
        // widget, and the user-only `changed` signal writes back.
        let number = NumberInput::new().with_range(1.0, 99.0).with_value(1.0);
        let quantity_subscription = number.changed().subscribe({
            let state = Rc::clone(&state);
            move |value| {
                state.quantity.set(*value);
            }
        });
        let number_value = number.value();
        runtime
            .append_child(runtime.root(), number)
            .expect("root exists");

        let button = Button::new();
        let enabled = button.enabled();
        let submit_subscription = button.activated().subscribe({
            let state = Rc::clone(&state);
            move |()| {
                state.submissions.set(state.submissions.get() + 1);
            }
        });
        let button = runtime
            .append_child(runtime.root(), button)
            .expect("root exists");
        runtime
            .append_child(
                button,
                Text::styled("Submit", TextStyle::new(16.0, Color::BLACK)),
            )
            .expect("button exists");

        // Property subscriptions derive validity; the binding drives the
        // widget. Both wake redraws without an application dirty flag.
        let mut subscriptions = vec![
            name_subscription,
            agreed_subscription,
            quantity_subscription,
            submit_subscription,
        ];
        subscriptions.push(state.name.subscribe({
            let state = Rc::clone(&state);
            move |_| state.revalidate()
        }));
        subscriptions.push(state.agreed.subscribe({
            let state = Rc::clone(&state);
            move |_| state.revalidate()
        }));
        let binding = runtime.bind(&enabled, &state.is_valid);
        let quantity_binding = runtime.bind(&number_value, &state.quantity);
        runtime.watch(&state.submissions);
        runtime.watch(&state.quantity);

        let size = Size::new(480.0, 360.0).expect("window size is valid");
        runtime
            .layout(Constraints::tight(size).expect("window constraints are valid"))
            .expect("layout succeeds");

        Self {
            runtime,
            size,
            _bindings: vec![binding, quantity_binding],
            _subscriptions: subscriptions,
        }
    }

    fn layout(&mut self, size: Size) {
        self.size = size;
        if let Ok(constraints) = Constraints::tight(size) {
            let _ = self.runtime.layout(constraints);
        }
    }
}

impl WindowApplication for BoundApp {
    fn window_options(&self) -> WindowOptions {
        WindowOptions::new("Drawen bindings", self.size)
    }

    fn attach_clipboard(&mut self, clipboard: Box<dyn Clipboard>) {
        self.runtime.set_clipboard(clipboard);
    }

    fn window_event(&mut self, event: WindowEvent) -> WindowAction {
        match event {
            WindowEvent::CloseRequested => WindowAction::Exit,
            WindowEvent::Resized(size) => {
                self.layout(size);
                WindowAction::RequestRedraw
            }
            WindowEvent::Input(event) => {
                let _ = self.runtime.dispatch_event(&event);
                // Watched and bound properties drain into the redraw request,
                // so no separate state dirty flag is needed here.
                if self.runtime.take_redraw_request() {
                    WindowAction::RequestRedraw
                } else {
                    WindowAction::None
                }
            }
            WindowEvent::Visible(_) | WindowEvent::Minimized(_) | WindowEvent::Occluded(_) => {
                WindowAction::None
            }
            WindowEvent::Maximized(_) | WindowEvent::Fullscreen(_) => WindowAction::RequestRedraw,
            WindowEvent::RedrawRequested => WindowAction::None,
        }
    }

    fn presentation_changed(&mut self, presentation: WindowPresentation) -> WindowAction {
        self.runtime.set_presented(presentation.is_presented());
        WindowAction::None
    }

    fn redraw(&mut self) -> DisplayList {
        let mut display_list = DisplayList::new();
        let _ = self
            .runtime
            .paint(&mut PaintContext::new(&mut display_list));
        display_list
    }

    fn tick(&mut self, delta: std::time::Duration) -> bool {
        self.runtime.tick(delta)
    }

    fn next_frame_in(&self) -> Option<std::time::Duration> {
        self.runtime.next_frame_in()
    }
}

fn available_size(constraints: Constraints) -> Size {
    let maximum = constraints.max();
    let minimum = constraints.min();
    constraints.constrain(
        Size::new(
            if maximum.width().is_finite() {
                maximum.width()
            } else {
                minimum.width()
            },
            if maximum.height().is_finite() {
                maximum.height()
            } else {
                minimum.height()
            },
        )
        .expect("layout dimensions are finite and non-negative"),
    )
}
