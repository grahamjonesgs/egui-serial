#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// hide console window on Windows in release
extern crate core;
extern crate csv;
extern crate preferences;
extern crate serde;

use std::cmp::max;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use std::time::Duration;

use eframe::egui::{vec2, Visuals};
use eframe::IconData;
use preferences::{AppInfo};

use crate::data::{DataContainer, Packet};
use crate::gui::{load_gui_settings, print_to_console, MyApp, Print, RIGHT_PANEL_WIDTH};

use crate::serial::{load_serial_settings, serial_thread, Device};

mod data;
mod gui;
mod serial;
mod toggle;

const APP_INFO: AppInfo = AppInfo {
    name: "Serial Monitor",
    author: "Graham Jones",
};
const PREFS_KEY: &str = "config/gui";
const PREFS_KEY_SERIAL: &str = "config/serial_devices";

fn split(payload: &str) -> Vec<f32> {
    let mut split_data: Vec<&str> = vec![];
    for s in payload.split(':') {
        split_data.extend(s.split(','));
    }
    split_data
        .iter()
        .map(|x| x.trim())
        .flat_map(|x| x.parse::<f32>())
        .collect()
}

fn main_thread(
    data_lock: Arc<RwLock<DataContainer>>,
    raw_data_rx: Receiver<Packet>,
    names_rx: Receiver<Vec<String>>,
    clear_rx: Receiver<bool>,
) {
    // reads data from mutex, samples and saves if needed
    let mut data = DataContainer::default();
    let mut failed_format_counter = 0;
    loop {
        if let Ok(cl) = clear_rx.recv_timeout(Duration::from_millis(1)) {
            if cl {
                data = DataContainer::default();
                failed_format_counter = 0;
            }
        }

        if let Ok(names) = names_rx.recv_timeout(Duration::from_millis(1)) {
            data.names = names;
        }

        if let Ok(packet) = raw_data_rx.recv_timeout(Duration::from_millis(1)) {
            if !packet.payload.is_empty() {
                data.raw_traffic.push(packet.clone());
                println!("xxxxx packet = {:?}", packet);
                let split_data = split(&packet.payload);
                if data.dataset.is_empty() || failed_format_counter > 10 {
                    // resetting dataset
                    data.dataset = vec![vec![]; max(split_data.len(), 1)];
                    if data.names.len() != split_data.len() {
                        data.names = (0..max(split_data.len(), 1))
                            .map(|i| format!("Column {i}"))
                            .collect();
                    }
                    failed_format_counter = 0;
                    // println!("resetting dataset. split length = {}, length data.dataset = {}", split_data.len(), data.dataset.len());
                } else if split_data.len() == data.dataset.len() {
                    // appending data
                    for (i, set) in data.dataset.iter_mut().enumerate() {
                        set.push(split_data[i]);
                        failed_format_counter = 0;
                    }
                    data.time.push(packet.relative_time);
                    data.absolute_time.push(packet.absolute_time);
                    if data.time.len() != data.dataset[0].len() {
                        // resetting dataset
                        data.time = vec![];
                        data.dataset = vec![vec![]; max(split_data.len(), 1)];
                        if data.names.len() != split_data.len() {
                            data.names = (0..max(split_data.len(), 1))
                                .map(|i| format!("Column {i}"))
                                .collect();
                        }
                    }
                } else {
                    // not same length
                    failed_format_counter += 1;
                    // println!("not same length in main! length split_data = {}, length data.dataset = {}", split_data.len(), data.dataset.len())
                }
                if let Ok(mut write_guard) = data_lock.write() {
                    *write_guard = data.clone();
                }
            }
        }

       

        // std::thread::sleep(Duration::from_millis(10));
    }
}

fn main() {
    let gui_settings = load_gui_settings();
    let saved_serial_device_configs = load_serial_settings();

    let device_lock = Arc::new(RwLock::new(Device::default()));
    let prog_counter_lock = Arc::new(RwLock::new(String::new()));
    let devices_lock = Arc::new(RwLock::new(vec![gui_settings.device.clone()]));
    let data_lock = Arc::new(RwLock::new(DataContainer::default()));
    let print_lock = Arc::new(RwLock::new(vec![Print::Empty]));
    let connected_lock = Arc::new(RwLock::new(false));

    let (send_tx, send_rx): (Sender<String>, Receiver<String>) = mpsc::channel();
    let (clear_tx, clear_rx): (Sender<bool>, Receiver<bool>) = mpsc::channel();
    let (names_tx, names_rx): (Sender<Vec<String>>, Receiver<Vec<String>>) = mpsc::channel();
    let (raw_data_tx, raw_data_rx): (Sender<Packet>, Receiver<Packet>) = mpsc::channel();

    let serial_device_lock = device_lock.clone();
    let serial_devices_lock = devices_lock.clone();
    let serial_print_lock = print_lock.clone();
    let serial_connected_lock = connected_lock.clone();
    let serial_prog_counter_lock = prog_counter_lock.clone();

    println!("starting connection thread..");
    let _serial_thread_handler = thread::spawn(|| {
        serial_thread(
            send_rx,
            raw_data_tx,
            serial_device_lock,
            serial_devices_lock,
            serial_print_lock,
            serial_connected_lock,
            serial_prog_counter_lock,
        );
    });

    let main_data_lock = data_lock.clone();

    println!("starting main thread..");
    let _main_thread_handler = thread::spawn(|| {
        main_thread(
            main_data_lock,
            raw_data_rx,
            names_rx,
            clear_rx,
        );
    });

    let options = eframe::NativeOptions {
        follow_system_theme: true,
        icon_data: Some(IconData::try_from_png_bytes(include_bytes!("../icons/icon.png")).unwrap()),
        drag_and_drop_support: true,
        initial_window_size: Option::from(vec2(gui_settings.x, gui_settings.y)),
        min_window_size: Option::from(vec2(2.0 * RIGHT_PANEL_WIDTH, 2.0 * RIGHT_PANEL_WIDTH)),
        ..Default::default()
    };

    let gui_data_lock = data_lock;
    let gui_device_lock = device_lock;
    let gui_devices_lock = devices_lock;
    let gui_connected_lock = connected_lock;
    let gui_print_lock = print_lock;
    let gui_prog_counter_lock = prog_counter_lock;

    //gui_prog_counter_lock.write().unwrap().push_str("test");

    if let Err(e) = eframe::run_native(
        "Serial Monitor",
        options,
        Box::new(|_cc| {
            _cc.egui_ctx.set_visuals(Visuals::dark());
            Box::new(MyApp::new(
                gui_print_lock,
                gui_data_lock,
                gui_device_lock,
                gui_devices_lock,
                saved_serial_device_configs,
                gui_connected_lock,
                gui_settings,
                names_tx,
                send_tx,
                clear_tx,
                gui_prog_counter_lock
                
            ))
        }),
    ) {
        println!("error: {e:?}");
    }
}
