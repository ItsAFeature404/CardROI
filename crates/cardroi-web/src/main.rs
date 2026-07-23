//! CardROI web entrypoint. Opens the real database through the async web
//! bridge - genuinely async, since IndexedDB itself is inherently async
//! in a browser - and shows a loading state until that resolves, so no
//! screen ever flashes empty/broken before the bridge is ready. Once the
//! bridge is ready, provides it as app-wide context and renders the
//! router.

use dioxus::prelude::*;

mod components;
mod local_prefs;
mod routes;
mod screens;
mod storage;
mod web_bridge;

use routes::Route;
use web_bridge::WebBridge;

// `dx` does not invoke the Tailwind compiler itself. After editing
// tokens.css/tailwind.css or adding a class in any component, regenerate
// from crates/cardroi-web/ with:
//   ./node_modules/.bin/tailwindcss -i assets/tailwind.css -o assets/tailwind.generated.css
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.generated.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let bridge = use_resource(WebBridge::open);

    rsx! {
        document::Title { "CardROI" }
        document::Stylesheet { href: TAILWIND_CSS }
        match &*bridge.read() {
            None => rsx! {
                div { class: "min-h-screen flex items-center justify-center bg-canvas text-text-primary font-data",
                    "Opening database…"
                }
            },
            Some(Err(err)) => rsx! {
                div { class: "min-h-screen flex items-center justify-center bg-canvas text-loss font-data p-8",
                    "Failed to open the browser database: {err}"
                }
            },
            Some(Ok(bridge)) => {
                use_context_provider(|| bridge.clone());
                rsx! {
                    Router::<Route> {}
                }
            }
        }
    }
}
