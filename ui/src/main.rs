#![allow(non_snake_case)]

use dioxus::prelude::*;

mod components;
mod constants;
#[cfg(feature = "example-data")]
mod example_data;
mod invites;
mod room_data;
mod util;

use components::app::App;

fn main() {
    dioxus::logger::initialize_default();
    launch(App);
}
