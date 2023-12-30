use std::env;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use serialport::{available_ports, ClearBuffer, FlowControl, SerialPort, SerialPortType};

enum MainState {
    Off,
    On,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Config {
    debug: bool,
    serial_number: String,
    millis_per_loop: u32,
    millis_hold: u32,
    threshold: u16,
    on_action: String,
    off_action: String,
}

fn get_value(port: &mut Box<dyn SerialPort>) -> Option<u16> {
    let mut out = None;
    let mut buf = [0; 2];
    
    port.clear(ClearBuffer::Input).ok();
    match port.write(&[1]) {
        Ok(_) => {
            let timeout = Instant::now();
            loop {
                if timeout.elapsed() >= Duration::from_millis(100) {
                    println!("timeout");
                    break;
                }
                if port.bytes_to_read().unwrap_or(0) >= 2 {
                    match port.read_exact(&mut buf) {
                        Ok(_) => { out = Some(u16::from_le_bytes(buf)) }
                        Err(_) => {}
                    }
                    break;
                }
                thread::sleep(Duration::from_millis(1));
            }
        }
        Err(_) => {}
    }
    
    out
}

fn main() {
    let config_path = env::args().nth(1).unwrap_or("".to_string());
    
    let config_raw = fs::read_to_string(&config_path).expect("Unable to open configuration file");
    let config: Config = serde_yaml::from_str(&config_raw).expect("Unable to parse configuration file");
    
    let product_name = Some("USB_Distance_Sensor".to_string());
    let mut sensor_port = None;
    
    let mut state = MainState::Off;
    let mut last_active_time = Instant::now();
    
    let mut off_action = Command::new("sh");
    off_action.arg("-c")
        .arg(&config.off_action);
    
    let mut on_action = Command::new("sh"); 
    on_action.arg("-c")
        .arg(&config.on_action);
    
    match available_ports() {
        Ok(ports) => {
            for p in ports {
                match p.port_type {
                    SerialPortType::UsbPort(info) => {
                        if config.debug { println!("port: {:?}", info) }
                        //if info.product == product_name && info.serial_number.unwrap_or("".to_string()) == config.serial_number {
                        if info.serial_number.unwrap_or("".to_string()) == config.serial_number {
                            let builder = serialport::new(&p.port_name, 115200);
                            let mut port = builder.open().unwrap();
                            port.set_flow_control(FlowControl::Hardware).ok();
                            sensor_port = Some(port);
                        }
                    }
                    SerialPortType::BluetoothPort => {},
                    SerialPortType::PciPort => {},
                    SerialPortType::Unknown => {},
                }
            }
        }
        Err(e) => {
            eprintln!("{:?}", e);
            eprintln!("Error listing serial ports");
        }
    }
    
    if sensor_port.is_some() {
        if config.debug { println!("loop start") }
        let mut sensor_inner = sensor_port.unwrap();
        loop {
            let loop_start = Instant::now();
            let current_value;
            match get_value(&mut sensor_inner) {
                Some(v) => { current_value = v }
                None => {
                    if config.debug { println!("get_value() failed") }
                    break;
                }
            }
            if config.debug { println!("value: {:?}", current_value) }
            
            match state {
                MainState::Off => {
                    if current_value <= config.threshold {
                        state = MainState::On;
                        last_active_time = Instant::now();
                        match on_action.output() {
                            Ok(_) => (),
                            Err(e) => {
                                if config.debug { println!("{}", e) }
                            }
                        }

                        if config.debug { println!("state off -> on") }
                    }
                }
                MainState::On => {
                    if current_value <= config.threshold {
                        last_active_time = Instant::now();
                    }
                    
                    let last_active_elapsed = u32::try_from(last_active_time.elapsed().as_millis())
                        .unwrap_or(u32::MAX);
                    if last_active_elapsed >= config.millis_hold {
                        state = MainState::Off;
                        match off_action.output() {
                            Ok(_) => (),
                            Err(e) => {
                                if config.debug { println!("{}", e) }
                            }
                        }
                        if config.debug { println!("state on -> off") }
                    }
                }
            }
            
            let delay_time = config.millis_per_loop.saturating_sub(loop_start.elapsed().as_millis() as u32);
            thread::sleep(Duration::from_millis(delay_time.into()));
        }
    }
}
