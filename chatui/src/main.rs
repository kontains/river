#![allow(non_snake_case)]

use dioxus::prelude::*;
use dioxus_web::Config;
use dioxus_logger::tracing::{Level, info};

mod components;
mod models;
use components::app::App;

fn main() {
    // Init logger
    dioxus_logger::init(Level::INFO).expect("failed to init logger");
    info!("starting app");

    dioxus_web::launch(App, |cfg| {
        cfg.with_root_name("app")
    });
}
