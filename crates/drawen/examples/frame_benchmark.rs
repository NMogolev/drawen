//! Reproducible headless frame benchmark.
//!
//! Run with `cargo run --release -p drawen --example frame_benchmark`.
//! The output reports p99, never just an average. It intentionally separates
//! retained UI work from software raster work: a GPU comparison needs a native
//! surface and must be collected by the platform-specific benchmark described
//! in `docs/PERFORMANCE.md`.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use drawen::{
    Color, Constraints, InputEvent, Modifiers, Point, Rect, Size, UiRuntime, VirtualDataSource,
    VirtualList, WheelDelta, WheelEvent, Widget,
    render::{DisplayList, FrameDamage, PaintContext},
    software::{PixelBuffer, SoftwareRenderer},
};

const SAMPLES: usize = 240;
const WINDOW_WIDTH: f32 = 1_280.0;
const WINDOW_HEIGHT: f32 = 800.0;

fn main() {
    println!("Drawen performance benchmark ({SAMPLES} measured frames; p99 after warmup)\n");
    retained_frame_benchmark();
    software_fill_benchmark(1_920, 1_080);
    software_fill_benchmark(3_840, 2_160);
    software_damage_benchmark(1_920, 1_080);
    runtime_local_damage_benchmark();
    virtual_list_scroll_benchmark();
}

fn retained_frame_benchmark() {
    let size = fixed_size(WINDOW_WIDTH, WINDOW_HEIGHT);
    let mut runtime = UiRuntime::new(FillScene);
    layout(&mut runtime, size);

    // Build the retained display list once. Subsequent frames must not repaint it.
    paint_frame(&mut runtime);
    let samples = measure(|| {
        paint_frame(&mut runtime);
    });
    let diagnostics = runtime.frame_diagnostics();

    println!("unchanged retained frame");
    print_timing(&samples);
    println!(
        "  retained paint {}  damage {}  commands {}",
        diagnostics.last_paint_reused(),
        describe_damage(diagnostics.last_damage()),
        diagnostics.last_paint_command_count(),
    );
    println!("  note: this includes appending the retained list to a fresh caller output list\n");
}

fn software_fill_benchmark(width: u32, height: u32) {
    let mut list = DisplayList::new();
    PaintContext::new(&mut list).fill_rect(
        Rect::new(
            Point::ZERO,
            fixed_size(u32_to_f32(width), u32_to_f32(height)),
        ),
        Color::rgba8(32, 96, 160, 255),
    );
    let renderer = SoftwareRenderer;
    let mut pixels =
        PixelBuffer::new(width, height).expect("benchmark buffer dimensions are valid");
    renderer.render(&list, &mut pixels).expect("fill renders");
    let samples = measure(|| {
        renderer.render(&list, &mut pixels).expect("fill renders");
    });
    let median = percentile(&samples, 50);
    let pixels_per_second = f64::from(width) * f64::from(height) / median.as_secs_f64();

    println!("software fill {width}x{height}");
    print_timing(&samples);
    println!(
        "  median fill rate {:.2} Gpixel/s\n",
        pixels_per_second / 1e9
    );
}

fn software_damage_benchmark(width: u32, height: u32) {
    // A dense grid of rounded fills and strokes gives the culler many commands
    // to skip when only a small region is damaged.
    let mut list = DisplayList::new();
    {
        let mut paint = PaintContext::new(&mut list);
        let cell = 40.0;
        let mut y = 0.0;
        while y < u32_to_f32(height) {
            let mut x = 0.0;
            while x < u32_to_f32(width) {
                let rect = Rect::new(
                    Point::new(x + 2.0, y + 2.0),
                    fixed_size(cell - 4.0, cell - 4.0),
                );
                paint.fill_rounded_rect(rect, 6.0, Color::rgba8(32, 96, 160, 255));
                paint.stroke_rounded_rect(rect, 6.0, 1.5, Color::rgba8(200, 220, 240, 255));
                x += cell;
            }
            y += cell;
        }
    }

    let renderer = SoftwareRenderer;
    let mut pixels =
        PixelBuffer::new(width, height).expect("benchmark buffer dimensions are valid");
    renderer.render(&list, &mut pixels).expect("scene renders");

    let full = measure(|| {
        renderer.render(&list, &mut pixels).expect("scene renders");
    });
    // A single damaged cell, grown by the effect margin the runtime uses.
    let region = Rect::new(Point::new(400.0, 300.0), fixed_size(88.0, 88.0));
    let damaged = measure(|| {
        renderer
            .render_with_damage(&list, &mut pixels, 1.0, FrameDamage::Region(region))
            .expect("scene renders");
    });
    let unchanged = measure(|| {
        renderer
            .render_with_damage(&list, &mut pixels, 1.0, FrameDamage::Unchanged)
            .expect("scene renders");
    });

    println!("software damage {width}x{height} ({} commands)", list.len());
    println!("  full repaint");
    print_timing(&full);
    println!("  one damaged region");
    print_timing(&damaged);
    println!("  unchanged frame");
    print_timing(&unchanged);
    let speedup = percentile(&full, 50).as_secs_f64() / percentile(&damaged, 50).as_secs_f64();
    println!("  median region speedup {speedup:.1}x\n");
}

fn runtime_local_damage_benchmark() {
    let size = fixed_size(WINDOW_WIDTH, WINDOW_HEIGHT);
    let mut runtime = UiRuntime::new(GridScene);
    layout(&mut runtime, size);
    let list = paint_frame(&mut runtime);

    let renderer = SoftwareRenderer;
    let mut pixels = PixelBuffer::new(1_280, 800).expect("benchmark buffer dimensions are valid");
    renderer.render(&list, &mut pixels).expect("scene renders");

    // Full invalidation rebuilds and repaints the whole window.
    let full = measure(|| {
        runtime.invalidate_paint();
        let list = paint_frame(&mut runtime);
        renderer.render(&list, &mut pixels).expect("scene renders");
    });
    // Local invalidation reports a region the backend can rasterize alone.
    let region = Rect::new(Point::new(400.0, 300.0), fixed_size(88.0, 88.0));
    let mut reported_region = false;
    let local = measure(|| {
        runtime.invalidate_rect(region);
        let list = paint_frame(&mut runtime);
        let damage = runtime.frame_damage();
        reported_region = matches!(damage, FrameDamage::Region(_));
        renderer
            .render_with_damage(&list, &mut pixels, 1.0, damage)
            .expect("scene renders");
    });

    println!("runtime local damage (grid scene, 1280x800)");
    println!("  full invalidate + repaint");
    print_timing(&full);
    println!("  invalidate_rect + damage repaint");
    print_timing(&local);
    println!(
        "  runtime reported a region: {reported_region}  speedup {:.1}x\n",
        percentile(&full, 50).as_secs_f64() / percentile(&local, 50).as_secs_f64()
    );
}

fn virtual_list_scroll_benchmark() {
    let size = fixed_size(WINDOW_WIDTH, WINDOW_HEIGHT);
    let mut runtime = UiRuntime::new(VirtualList::new(BenchmarkRows(1_000_000), 28.0));
    layout(&mut runtime, size);
    paint_frame(&mut runtime);

    let mut text_commands = 0;
    let samples = measure(|| {
        let scroll = InputEvent::Wheel(WheelEvent {
            position: Point::new(10.0, 10.0),
            delta: WheelDelta::Pixels(Point::new(0.0, -28.0)),
            modifiers: Modifiers::NONE,
        });
        runtime.dispatch_event(&scroll);
        layout(&mut runtime, size);
        let list = paint_frame(&mut runtime);
        text_commands = list
            .commands()
            .iter()
            .filter(|command| matches!(command, drawen::render::DisplayCommand::DrawText { .. }))
            .count();
    });

    println!("virtual list scroll (1,000,000 rows)");
    print_timing(&samples);
    println!(
        "  visible row text commands {text_commands} (must scale with viewport, not row count)\n"
    );
}

fn layout(runtime: &mut UiRuntime, size: Size) {
    runtime
        .layout(Constraints::tight(size).expect("benchmark constraints are valid"))
        .expect("benchmark scene lays out");
}

fn paint_frame(runtime: &mut UiRuntime) -> DisplayList {
    let mut display_list = DisplayList::new();
    runtime
        .paint(&mut PaintContext::new(&mut display_list))
        .expect("benchmark scene paints");
    black_box(display_list.len());
    display_list
}

fn measure(mut frame: impl FnMut()) -> Vec<Duration> {
    for _ in 0..16 {
        frame();
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        frame();
        samples.push(started.elapsed());
    }
    samples
}

fn print_timing(samples: &[Duration]) {
    println!(
        "  median {:>8.2} us  p99 {:>8.2} us  worst {:>8.2} us",
        microseconds(percentile(samples, 50)),
        microseconds(percentile(samples, 99)),
        microseconds(*samples.iter().max().expect("samples are non-empty")),
    );
}

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let index = (percentile * (sorted.len() - 1)).div_ceil(100);
    sorted[index]
}

fn describe_damage(damage: Option<Rect>) -> &'static str {
    if damage.is_some() { "present" } else { "none" }
}

fn microseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

struct FillScene;

impl Widget for FillScene {
    fn layout(
        &mut self,
        _: &mut drawen::LayoutContext<'_>,
        constraints: Constraints,
    ) -> drawen::LayoutResult {
        drawen::LayoutResult::new(constraints.constrain(fixed_size(WINDOW_WIDTH, WINDOW_HEIGHT)))
    }

    fn style_target(&self) -> drawen::StyleTarget {
        drawen::StyleTarget::new("benchmark-fill")
    }

    fn paint(&self, context: &mut PaintContext<'_>, _: &drawen::UiEnvironment, bounds: Rect) {
        context.fill_rect(bounds, Color::rgba8(32, 96, 160, 255));
    }
}

struct GridScene;

impl Widget for GridScene {
    fn layout(
        &mut self,
        _: &mut drawen::LayoutContext<'_>,
        constraints: Constraints,
    ) -> drawen::LayoutResult {
        drawen::LayoutResult::new(constraints.constrain(fixed_size(WINDOW_WIDTH, WINDOW_HEIGHT)))
    }

    fn style_target(&self) -> drawen::StyleTarget {
        drawen::StyleTarget::new("benchmark-grid")
    }

    fn paint(&self, context: &mut PaintContext<'_>, _: &drawen::UiEnvironment, bounds: Rect) {
        context.fill_rect(bounds, Color::rgba8(16, 20, 26, 255));
        let cell = 40.0;
        let mut y = bounds.origin.y;
        while y + cell <= bounds.bottom() {
            let mut x = bounds.origin.x;
            while x + cell <= bounds.right() {
                let rect = Rect::new(
                    Point::new(x + 2.0, y + 2.0),
                    fixed_size(cell - 4.0, cell - 4.0),
                );
                context.fill_rounded_rect(rect, 6.0, Color::rgba8(32, 96, 160, 255));
                context.stroke_rounded_rect(rect, 6.0, 1.5, Color::rgba8(200, 220, 240, 255));
                x += cell;
            }
            y += cell;
        }
    }
}

struct BenchmarkRows(usize);

impl VirtualDataSource for BenchmarkRows {
    fn row_count(&self) -> usize {
        self.0
    }

    fn row_key(&self, row: usize) -> u64 {
        row as u64
    }

    fn row_text(&self, row: usize) -> String {
        format!("Item {row:06} — cached row visual")
    }
}

fn fixed_size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("benchmark dimensions are finite")
}

fn u32_to_f32(value: u32) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    {
        value as f32
    }
}
