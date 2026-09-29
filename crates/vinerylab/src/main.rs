use misina_lab::Generator;
use vinerylab::Vineyard;

fn main() {
    let mut app = misina_lab::viewer::app::<Vineyard>();
    // Viewer-only: gizmos need `GizmoPlugin`, which the headless path's
    // `MinimalPlugins` does not provide.
    app.add_plugins(vinerylab::elements::util::parcel::debug_plugin);
    // The per-layer half of `misina_lab::perf`, under the same variable.
    if Vineyard::env("PERF").is_some() {
        app.add_plugins(vinerylab::perf::plugin);
    }
    app.run();
}
