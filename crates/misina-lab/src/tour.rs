//! Plays a storyboard in the viewer — camera moves, panel scrolls, parameter
//! edits — for recording a demo video with [`record`](crate::record):
//!
//! ```text
//! BOXLAB_TOUR=tour.json BOXLAB_RECORD=demo.mp4 cargo run --release
//! ```
//!
//! The variable is `{PACKAGE}_TOUR`, naming a JSON file in [`Tour`]'s shape.
//! While recording, the clock advances one video frame per rendered frame, so
//! the video does not depend on how fast the machine draws: a rebuild that
//! stalls the window for a second costs one frame, and a camera move is as
//! smooth on a software renderer as on a GPU. Without `{PACKAGE}_RECORD` the
//! tour plays on the wall clock instead, as a preview.
//!
//! The tour changes what a person would — the panel's [`Staged`] params, the
//! orbit camera, the panel's scroll — so the viewer answers as it does to a
//! person, the settle before an edit reaches the scene included. It sizes the
//! window to the video, takes the pointer away from it, and quits the app
//! after the last step, which finishes the file.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, bail, ensure};
use bevy::math::curve::{Curve, EaseFunction};
use bevy::picking::PickingSettings;
use bevy::prelude::*;
use bevy::reflect::NamedField;
use bevy::render::view::screenshot::Screenshot;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::{Checked, ComputedNode, ScrollPosition, UiGlobalTransform};
use bevy::ui_widgets::{Checkbox, ScrollArea, SliderPrecision, SliderValue, ValueChange};
use bevy::window::{CursorOptions, PrimaryWindow, WindowResolution};
use bevy_panorbit_camera::PanOrbitCamera;
use serde::Deserialize;
use serde_json::Value;

use crate::params::{self, Widget};
use crate::scene::z_up_to_y_up;
use crate::terrain::Ground;
use crate::ui::{Bound, Section, Staged, Tip, round, tip_popup};
use crate::{Generator, Params, record};

/// Plays the storyboard `{PACKAGE}_TOUR` names.
pub fn plugin<G: Generator>(app: &mut App) {
    let Some(path) = G::env("TOUR") else {
        return;
    };
    let tour = load::<G::Params>(Path::new(&path))
        .unwrap_or_else(|err| panic!("{}: {err:#}", path.display()));

    let world = app.world_mut();
    let (mut window, mut cursor) = world
        .query_filtered::<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>()
        .single_mut(world)
        .expect("the viewer opens one window");
    window.resolution =
        WindowResolution::new(tour.size[0], tour.size[1]).with_scale_factor_override(tour.scale);
    // The recorder stops at a resize, since ffmpeg takes one frame size.
    window.resizable = false;
    // A hover would float a tooltip into the shot, and a drag would turn the
    // camera: the pointer is cut off from the window, and from picking too in
    // case the window system lets it through anyway.
    cursor.hit_test = false;
    app.insert_resource(PickingSettings {
        is_input_enabled: false,
        ..default()
    });

    if G::env("RECORD").is_some() {
        // A nanosecond over the period: the recorder captures once a whole
        // period has passed by its f64 sums, which a step of exactly one
        // would sometimes fall short of, skipping a frame.
        let frame = Duration::from_nanos((1e9 / record::fps::<G>()) as u64 + 1);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
    }
    app.insert_resource(record::Hold)
        .insert_resource(tour)
        .add_systems(Update, play::<G::Params>);
}

/// A storyboard: the window it is shot in, the opening shot, and the steps
/// that follow it.
#[derive(Resource, Deserialize, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Tour {
    /// The window's size in physical pixels, which is the video's.
    pub size: [u32; 2],
    /// Physical pixels per logical one. Above 1 enlarges the panel, which
    /// keeps its text legible in a video played smaller than it was recorded.
    pub scale: f32,
    /// Wall-clock seconds the opening shot gets to build and compile its
    /// shaders before recording starts. A slow machine may need more.
    pub warmup: f32,
    /// Played during the warmup, so recording opens on it. Its `wait` is
    /// ignored.
    pub start: Step,
    pub steps: Vec<Step>,
}

impl Default for Tour {
    fn default() -> Self {
        Self {
            size: [1280, 720],
            scale: 1.0,
            warmup: 3.0,
            start: Step::default(),
            steps: Vec::new(),
        }
    }
}

/// One beat. Everything in a step starts together and takes `secs`.
#[derive(Deserialize, Default, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Step {
    /// Seconds its moves take; zero is a cut.
    pub secs: f32,
    /// Seconds from this step's start to the next one's, `secs` if left out:
    /// shorter overlaps the next step, longer holds the shot.
    pub wait: Option<f32>,
    /// Where the camera goes. Parts left out stay as they are.
    pub camera: Option<Pose>,
    /// Parameters by `fragment.field`, as Python names them. A number slides
    /// there over `secs`; a flag or a name switches when the step starts.
    /// The value has to be one the panel offers.
    pub set: BTreeMap<String, Value>,
    /// A panel field, `fragment.field`, to bring into view: its section
    /// opens, the others fold, and the panel scrolls it to the middle over
    /// `secs`. Its tooltip floats beside it until a step neither shows nor
    /// sets it — a step that only moves the camera included.
    pub show: Option<String>,
    /// Checkboxes outside the params, such as the footer's view options, by
    /// caption: ticked or not when the step starts.
    pub toggle: BTreeMap<String, bool>,
    /// Ignored: what the step is for, JSON having no comments.
    pub note: String,
}

/// Where the orbit camera is, in the terms a person orbits it in.
#[derive(Deserialize, Default, Clone, Copy, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Pose {
    /// The point orbited and looked at, in meters along the scene's X and Y
    /// and then up from the ground there — so a close-up stays on the objects
    /// whatever the hills do. The ground is read when the step starts.
    pub focus: Option<[f32; 3]>,
    /// Degrees around the vertical. 0 puts the camera on the focus's −Y side,
    /// 90 on its +X side.
    pub yaw: Option<f32>,
    /// Degrees above the horizon.
    pub pitch: Option<f32>,
    /// Meters from the focus.
    pub radius: Option<f32>,
}

/// Reads a storyboard, and checks every parameter it names against `P`.
pub fn load<P: Params>(path: &Path) -> anyhow::Result<Tour> {
    parse::<P>(&std::fs::read_to_string(path)?)
}

fn parse<P: Params>(json: &str) -> anyhow::Result<Tour> {
    let tour: Tour = serde_json::from_str(json)?;
    for (i, step) in std::iter::once(&tour.start).chain(&tour.steps).enumerate() {
        // `start` is step 0 and the first of `steps` step 1, as the log
        // counts them.
        let context = || format!("step {i}");
        for (path, value) in &step.set {
            check(field::<P>(path).with_context(context)?, path, value).with_context(context)?;
        }
        if let Some(path) = &step.show {
            field::<P>(path).with_context(context)?;
        }
    }
    Ok(tour)
}

/// The declaration of the field `path` names.
fn field<P: Params>(path: &str) -> anyhow::Result<&'static NamedField> {
    let (fragment, field) = path
        .split_once('.')
        .with_context(|| format!("`{path}` is not `fragment.field`"))?;
    params::fragments::<P>()
        .find(|f| f.name() == fragment)
        .and_then(|f| params::fields(f).find(|f| f.name() == field))
        .with_context(|| format!("no parameter `{path}`"))
}

/// Whether `value` is one the field's control can show.
fn check(field: &NamedField, path: &str, value: &Value) -> anyhow::Result<()> {
    match (params::widget(field), value) {
        (Widget::Slider(slider), Value::Number(number)) => {
            let number = number.as_f64().unwrap_or(f64::NAN) as f32;
            ensure!(
                (slider.min..=slider.max).contains(&number),
                "`{path}`: {number} is off the slider, {} to {}",
                slider.min,
                slider.max,
            );
        }
        (Widget::Checkbox, Value::Bool(_)) => {}
        (Widget::Dropdown(names), Value::String(name)) => {
            ensure!(
                names.contains(&name.as_str()),
                "`{path}`: `{name}` is not one of {names:?}",
            );
        }
        (widget, _) => bail!("`{path}` is a {widget:?}, which takes no {value}"),
    }
    Ok(())
}

/// Where the tour is.
#[derive(Default)]
struct Playhead {
    phase: Phase,
    /// When the first step started, on `Time<Real>`'s clock.
    zero: f64,
    camera: Option<Tween<(Orbit, Orbit)>>,
    /// The control the panel is scrolling to, its scroll area, and where the
    /// scroll started.
    scroll: Option<Tween<(Entity, Entity, f32)>>,
    /// The parameter, and the values it slides between.
    sliding: Vec<Tween<(String, f32, f32)>>,
    /// The field whose tooltip is up, and the card showing it.
    tip: Option<(String, Entity)>,
}

#[derive(Default)]
enum Phase {
    /// Waiting for the orbit camera to set itself up.
    #[default]
    Opening,
    /// The opening shot settling, since this wall-clock instant.
    Settling(Instant),
    /// Recording: the step that starts next, and when.
    Playing { next: usize, due: f64 },
    /// Past the last step, waiting for the frames still being read back.
    Ending,
}

/// A move under way. The camera's and the scroll's stay once done, holding
/// their end until a step replaces them; a parameter's ends, since writing
/// the staged params every frame would keep them from ever settling.
struct Tween<T> {
    began: f64,
    secs: f32,
    what: T,
}

impl<T> Tween<T> {
    /// How far along it is at `now`, eased: 0 at its start, 1 from its end on.
    fn progress(&self, now: f64) -> f32 {
        let t = if self.secs > 0.0 {
            ((now - self.began) / self.secs as f64) as f32
        } else {
            1.0
        };
        EaseFunction::SmoothStep.sample_clamped(t)
    }

    fn done(&self, now: f64) -> bool {
        now - self.began >= self.secs as f64
    }
}

/// A whole orbit pose, in Bevy's Y-up world and radians.
#[derive(Clone, Copy)]
struct Orbit {
    focus: Vec3,
    yaw: f32,
    pitch: f32,
    radius: f32,
}

impl Orbit {
    fn of(camera: &PanOrbitCamera) -> Self {
        Self {
            focus: camera.focus,
            yaw: camera.yaw.unwrap_or_default(),
            pitch: camera.pitch.unwrap_or_default(),
            radius: camera.radius.unwrap_or(1.0),
        }
    }

    /// This pose with the parts `pose` names replaced.
    fn with(self, pose: Pose, ground: Option<&Ground>) -> Self {
        let focus = |[x, y, up]: [f32; 3]| {
            let z = ground.map_or(0.0, |ground| ground.height(x, y)) + up;
            z_up_to_y_up() * Vec3::new(x, y, z)
        };
        Self {
            focus: pose.focus.map_or(self.focus, focus),
            yaw: pose.yaw.map_or(self.yaw, f32::to_radians),
            pitch: pose.pitch.map_or(self.pitch, f32::to_radians),
            radius: pose.radius.unwrap_or(self.radius),
        }
    }

    fn lerp(self, to: Self, t: f32) -> Self {
        Self {
            focus: self.focus.lerp(to.focus, t),
            yaw: self.yaw.lerp(to.yaw, t),
            pitch: self.pitch.lerp(to.pitch, t),
            // Geometric, so a dolly from 100 m to 5 m covers the same share
            // of the way every frame instead of crawling in at the end.
            radius: self.radius * (to.radius / self.radius).powf(t),
        }
    }

    /// Puts the camera here this frame, past its smoothing: target and
    /// current pose both, and a forced update since they already agree.
    fn place(self, camera: &mut PanOrbitCamera) {
        camera.focus = self.focus;
        camera.target_focus = self.focus;
        camera.yaw = Some(self.yaw);
        camera.target_yaw = self.yaw;
        camera.pitch = Some(self.pitch);
        camera.target_pitch = self.pitch;
        camera.radius = Some(self.radius);
        camera.target_radius = self.radius;
        camera.force_update = true;
    }
}

fn play<P: Params>(world: &mut World, mut at: Local<Playhead>) {
    let at = &mut *at;
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    world.resource_scope(|world, tour: Mut<Tour>| {
        match at.phase {
            // The orbit camera reads its pose off its transform on its first
            // frame, over whatever was set before.
            Phase::Opening if camera(world).initialized => {
                begin::<P>(world, at, &tour.start, now);
                at.phase = Phase::Settling(Instant::now());
            }
            Phase::Settling(since) if since.elapsed().as_secs_f32() >= tour.warmup => {
                world.remove_resource::<record::Hold>();
                at.zero = now;
                at.phase = Phase::Playing { next: 0, due: now };
            }
            _ => {}
        }
        while let Phase::Playing { next, due } = at.phase
            && now >= due
        {
            match tour.steps.get(next) {
                Some(step) => {
                    info!("tour: step {} at {:.2} s", next + 1, due - at.zero);
                    begin::<P>(world, at, step, now);
                    let due = due + step.wait.unwrap_or(step.secs) as f64;
                    at.phase = Phase::Playing {
                        next: next + 1,
                        due,
                    };
                }
                None => {
                    world.insert_resource(record::Hold);
                    at.phase = Phase::Ending;
                }
            }
        }
        animate::<P>(world, at, now);
        if matches!(at.phase, Phase::Ending)
            && world.query::<&Screenshot>().iter(world).next().is_none()
        {
            world.write_message(AppExit::Success);
        }
    });
}

/// Starts a step: sets its moves going and makes its cuts.
fn begin<P: Params>(world: &mut World, at: &mut Playhead, step: &Step, now: f64) {
    let (began, secs) = (now, step.secs);
    if let Some(pose) = step.camera {
        let from = Orbit::of(&camera(world));
        at.camera = Some(Tween {
            began,
            secs,
            what: (from, from.with(pose, world.get_resource::<Ground>())),
        });
    }
    for (path, value) in &step.set {
        let (fragment, field) = path.split_once('.').expect("checked by `load`");
        let mut staged = world.resource_mut::<Staged<P>>();
        let field = params::get_mut(&mut staged.0, fragment, field).expect("checked by `load`");
        match value {
            Value::Number(to) => {
                let from = params::number(field).expect("checked by `load`");
                at.sliding.retain(|tween| tween.what.0 != *path);
                at.sliding.push(Tween {
                    began,
                    secs,
                    what: (path.clone(), from, to.as_f64().unwrap_or_default() as f32),
                });
            }
            Value::Bool(flag) => field.apply(flag),
            Value::String(name) => field.apply(name),
            _ => unreachable!("checked by `load`"),
        }
    }
    if let Some(path) = &step.show {
        let fragment = path.split('.').next().unwrap_or_default();
        let toggles: Vec<_> = world
            .query::<(Entity, &Section, Has<Checked>)>()
            .iter(world)
            .filter(|(_, section, open)| (section.0 == fragment) != *open)
            .map(|(toggle, section, _)| (toggle, section.0 == fragment))
            .collect();
        for (toggle, open) in toggles {
            world.trigger(ValueChange {
                source: toggle,
                value: open,
                is_final: true,
            });
        }
        let control = control(world, path);
        let area = up::<ScrollArea>(world, control).expect("the panel scrolls");
        let from = scrolled(world, area);
        at.scroll = Some(Tween {
            began,
            secs,
            what: (control, area, from),
        });
    }
    for (caption, on) in &step.toggle {
        let mut texts = world.query::<(Entity, &Text)>();
        let checkbox = texts
            .iter(world)
            .filter(|(_, text)| text.0 == *caption)
            .find_map(|(text, _)| up::<Checkbox>(world, text))
            .unwrap_or_else(|| panic!("no checkbox is captioned `{caption}`"));
        if world.entity(checkbox).contains::<Checked>() != *on {
            world.trigger(ValueChange {
                source: checkbox,
                value: *on,
                is_final: true,
            });
        }
    }
    let touched = |path: &String| step.show.as_ref() == Some(path) || step.set.contains_key(path);
    if !at.tip.as_ref().is_some_and(|(path, _)| touched(path)) {
        if let Some((_, card)) = at.tip.take() {
            world.despawn(card);
        }
        if let Some(path) = &step.show {
            let control = control(world, path);
            let text = world
                .get::<Tip>(control)
                .expect("found by its `Tip`")
                .0
                .clone();
            let card = world
                .spawn_scene(tip_popup(text, false))
                .expect("a tooltip spawns")
                .insert(ChildOf(control))
                .id();
            at.tip = Some((path.clone(), card));
        }
    }
}

/// Moves everything under way to where it is at `now`.
fn animate<P: Params>(world: &mut World, at: &mut Playhead, now: f64) {
    if let Some(tween) = &at.camera {
        let (from, to) = tween.what;
        from.lerp(to, tween.progress(now)).place(&mut camera(world));
    }
    // Not on the frame the scroll began: a section it unfolded is laid out
    // at the end of that frame, so its controls have no place yet.
    if let Some(tween) = at.scroll.as_ref().filter(|tween| now > tween.began) {
        let (control, area, from) = tween.what;
        let control_y = world
            .get::<UiGlobalTransform>(control)
            .map(|g| g.translation.y);
        let area_y = world
            .get::<UiGlobalTransform>(area)
            .map(|g| g.translation.y);
        if let (Some(control_y), Some(area_y), Some(node)) =
            (control_y, area_y, world.get::<ComputedNode>(area).cloned())
        {
            // The scroll that puts the control in the middle, worked out
            // afresh each frame as the layout follows the folding. From the
            // scroll the layout applied, which a fold may have clamped below
            // the one asked for.
            let scale = node.inverse_scale_factor;
            let max = (node.content_size.y - node.size.y + node.scrollbar_size.y).max(0.0) * scale;
            let target = scrolled(world, area) + (control_y - area_y) * scale;
            let y = from
                .min(max)
                .lerp(target.clamp(0.0, max), tween.progress(now));
            world
                .entity_mut(area)
                .insert(ScrollPosition(Vec2::new(0.0, y)));
        }
    }
    at.sliding.retain(|tween| {
        let (path, from, to) = &tween.what;
        // Rounded as a drag rounds it, and shown on the slider as a drag
        // would show it.
        let slider = control(world, path);
        let precision = world.get::<SliderPrecision>(slider).map_or(3, |p| p.0);
        let value = round(from.lerp(*to, tween.progress(now)), precision);
        let (fragment, field) = path.split_once('.').expect("checked by `load`");
        let mut staged = world.resource_mut::<Staged<P>>();
        if let Some(field) = params::get_mut(&mut staged.0, fragment, field) {
            params::set_number(field, value);
        }
        world.entity_mut(slider).insert(SliderValue(value));
        !tween.done(now)
    });
}

/// How far `area` is scrolled as laid out, in logical pixels.
fn scrolled(world: &World, area: Entity) -> f32 {
    world.get::<ComputedNode>(area).map_or(0.0, |node| {
        node.scroll_position.y * node.inverse_scale_factor
    })
}

fn camera(world: &mut World) -> Mut<'_, PanOrbitCamera> {
    world
        .query::<&mut PanOrbitCamera>()
        .single_mut(world)
        .expect("the viewer has one orbit camera")
}

/// The panel control editing `fragment.field`: the node carrying its tooltip.
fn control(world: &mut World, path: &str) -> Entity {
    let (fragment, field) = path.split_once('.').expect("checked by `load`");
    let mut bound = world.query::<(Entity, &Bound)>();
    bound
        .iter(world)
        .filter(|(_, bound)| bound.fragment == fragment && bound.field == field)
        .find_map(|(entity, _)| up::<Tip>(world, entity))
        .unwrap_or_else(|| panic!("no control edits `{path}`"))
}

/// The nearest of `entity` and its ancestors that has a `T`.
fn up<T: Component>(world: &World, entity: Entity) -> Option<Entity> {
    std::iter::successors(Some(entity), |e| {
        world.get::<ChildOf>(*e).map(ChildOf::parent)
    })
    .find(|e| world.entity(*e).contains::<T>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixture::BoxesParams;

    /// A storyboard may name only parameters there are, set to values their
    /// controls offer; anything else fails before the window opens, saying
    /// which step.
    #[test]
    fn a_storyboard_is_checked_against_the_params() {
        let tour = |set: &str| {
            parse::<BoxesParams>(&format!(
                r#"{{"steps": [{{"show": "boxes.gap"}}, {{"set": {set}}}]}}"#
            ))
        };
        tour(r#"{"boxes.count": 8, "boxes.material": "stone", "boxes.open": true}"#).unwrap();
        for (set, error) in [
            (r#"{"boxes.colour": 1}"#, "no parameter `boxes.colour`"),
            (r#"{"boxes.count": 9}"#, "off the slider"),
            (r#"{"boxes.material": "glass"}"#, "not one of"),
            (r#"{"boxes.open": 1}"#, "takes no 1"),
        ] {
            let err = format!("{:#}", tour(set).unwrap_err());
            assert!(err.contains("step 2") && err.contains(error), "{err}");
        }
    }
}
