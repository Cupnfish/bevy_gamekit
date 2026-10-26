//! Rewarded-video ads for TapTap H5.
//!
//! - Web: `TapBridge.showRewarded(cb)` from the packaged glue — the real H5
//!   SDK when `window.TAP_AD_UNIT_ID` is configured, a simulated grant
//!   otherwise (see `runtime/taptap-glue.js`).
//! - Native: a background thread simulates the grant after a short delay so
//!   the full reward flow is testable locally.
//!
//! Callbacks are non-`Send` on the JS single thread, so results are buffered
//! in a channel and drained once per frame by [`poll_ad_events_system`];
//! register it in `PreUpdate`/`Update` and read [`AdEvent`].

#[cfg(feature = "bevy")]
mod imp {
    use bevy_ecs::message::{Message, MessageWriter};

    /// Result of one rewarded-video request.
    #[derive(Clone, Copy, Debug, Message)]
    pub struct AdEvent {
        /// True only when the video was watched to the end.
        pub granted: bool,
    }

    /// Request a rewarded video (fire-and-forget; the outcome arrives as an
    /// [`AdEvent`] on a later frame).
    pub fn show_rewarded() {
        platform::show_rewarded();
    }

    /// Drain bridge results into Bevy messages, once per frame.
    pub fn poll_ad_events_system(mut events: MessageWriter<AdEvent>) {
        for event in platform::poll_ad_events() {
            events.write(event);
        }
    }

    #[cfg(target_arch = "wasm32")]
    mod platform {
        use super::*;
        use std::cell::RefCell;
        use std::sync::mpsc::{channel, Receiver, Sender};
        use std::sync::{Mutex, OnceLock};
        use wasm_bindgen::prelude::*;
        use wasm_bindgen::JsValue;

        static EVENTS: OnceLock<Mutex<(Sender<AdEvent>, Receiver<AdEvent>)>> = OnceLock::new();
        // JS is single-threaded and Closures are !Send: park them in a
        // thread_local so they outlive the call while staying on this thread.
        thread_local! {
            static CALLBACKS: RefCell<Vec<Closure<dyn FnMut(JsValue)>>> =
                const { RefCell::new(Vec::new()) };
        }

        fn events() -> &'static Mutex<(Sender<AdEvent>, Receiver<AdEvent>)> {
            EVENTS.get_or_init(|| Mutex::new(channel()))
        }

        pub fn show_rewarded() {
            let (tx, rx) = channel::<AdEvent>();
            let closure = Closure::new(move |granted: JsValue| {
                let _ = tx.send(AdEvent {
                    granted: granted.is_truthy(),
                });
            });
            let js_cb: JsValue = closure.as_ref().clone();
            CALLBACKS.with(|cell| cell.borrow_mut().push(closure));
            crate::bridge::show_rewarded_raw(&js_cb);
        }

        pub fn poll_ad_events() -> Vec<AdEvent> {
            let mut out = Vec::new();
            if let Ok(guard) = events().lock() {
                while let Ok(event) = guard.1.try_recv() {
                    out.push(event);
                }
            }
            out
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod platform {
        use super::*;
        use std::sync::Mutex;

        static RESULTS: Mutex<Vec<AdEvent>> = Mutex::new(Vec::new());

        /// Desktop simulation: grant the reward after a short delay so the
        /// whole flow can be exercised locally.
        pub fn show_rewarded() {
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(500));
                RESULTS
                    .lock()
                    .unwrap()
                    .push(AdEvent { granted: true });
            });
        }

        pub fn poll_ad_events() -> Vec<AdEvent> {
            RESULTS.lock().unwrap().drain(..).collect()
        }
    }
}

#[cfg(feature = "bevy")]
pub use imp::*;
