use pixels::{Pixels, SurfaceTexture};
use std::sync::Arc;
use std::time::Instant;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

const MEM_LEN: usize = 4096;
const START_ADDR: usize = 0x200;
const DISPLAY_WIDTH: usize = 64;
const DISPLAY_HEIGHT: usize = 32;

struct App {
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    crashed: bool,
    memory: [u8; MEM_LEN],
    display: [u8; DISPLAY_WIDTH * DISPLAY_HEIGHT],
    stack: Vec<u16>,
    dt: u16,
    st: u16,
    pc: usize,
    i: u16,
    v: [u16; 16],
    keys: [bool; 16],
    last_timer_update: Instant,
}

impl App {
    fn new() -> Self {
        let mut app = Self {
            window: None,
            pixels: None,
            crashed: false,
            memory: [0; MEM_LEN],
            display: [0; DISPLAY_WIDTH * DISPLAY_HEIGHT],
            stack: Vec::new(),
            dt: 0,
            st: 0,
            pc: START_ADDR,
            i: 0,
            v: [0; 16],
            keys: [false; 16],
            last_timer_update: Instant::now(),
        };

        load_binary(&mut app.memory, "./src/si.ch8");
        app
    }

    fn step_cpu(&mut self) -> Result<(), String> {
        let opcode = fetch_opcode(&self.memory, self.pc)?;

        match opcode & 0xF000 {
            0x0000 => match opcode & 0x0FFF {
                0x00E0 => {
                    self.display = [0; DISPLAY_WIDTH * DISPLAY_HEIGHT];
                    self.pc += 2;
                }
                0x00EE => {
                    if let Some(addr) = self.stack.pop() {
                        self.pc = usize::from(addr);
                    } else {
                        panic!(
                            "Line {}: Couldn't return from subroutine. No address available in the stack: code: {:04X}",
                            self.pc - START_ADDR,
                            opcode
                        );
                    }
                }
                _ => panic!(
                    "Line {}: Unsupported instruction 0NNN: {:x}\nDid you mean:\n- 0x00E0 CLS (clears the display) or,\n- 0x00EE RET (Return from subroutine).",
                    self.pc - START_ADDR,
                    opcode
                ),
            },
            0x1000 => {
                println!("Jump instruction to {:X}", opcode & 0x0FFF);
                self.pc = usize::from(opcode & 0x0FFF);
            }
            0x2000 => {
                let addr = opcode & 0x0FFF;
                self.stack.push((self.pc + 2) as u16);
                self.pc = addr as usize;
            }
            0x3000 => {
                let idx = (opcode & 0x0F00) >> 8;
                let val = opcode & 0x00FF;

                if self.v[idx as usize] == val {
                    self.pc += 4;
                } else {
                    self.pc += 2;
                }
            }
            0x4000 => {
                let idx = (opcode & 0x0F00) >> 8;
                let val = opcode & 0x00FF;

                if self.v[idx as usize] != val {
                    self.pc += 4;
                } else {
                    self.pc += 2;
                }
            }
            0x6000 => {
                let idx = ((opcode & 0x0F00) >> 8) as usize;
                let val = opcode & 0x00FF;
                println!("Set V[{}] to {}", idx, val);

                self.v[idx] = val;
                self.pc += 2;
            }
            0x7000 => {
                let idx = ((opcode & 0x0F00) >> 8) as usize;
                let val = opcode & 0x00FF;

                self.v[idx] += val;
                self.pc += 2;
            }
            0x8000 => match opcode & 0x000F {
                0x0000 => {
                    let idx = (opcode & 0x0F00) >> 8;
                    let idy = (opcode & 0x00F0) >> 4;

                    self.v[idx as usize] = self.v[idy as usize];
                    self.pc += 2;
                }
                0x0002 => {
                    let idx = (opcode & 0x0F00) >> 8;
                    let idy = (opcode & 0x00F0) >> 4;

                    self.v[idx as usize] &= self.v[idy as usize];
                    self.pc += 2;
                }
                0x0003 => {
                    let idx = (opcode & 0x0F00) >> 8;
                    let idy = (opcode & 0x00F0) >> 4;

                    self.v[idx as usize] ^= self.v[idy as usize];
                    self.pc += 2;
                }
                0x0004 => {
                    let idx = (opcode & 0x0F00) >> 8;
                    let idy = (opcode & 0x00F0) >> 4;

                    self.v[idx as usize] += self.v[idy as usize];

                    if self.v[idx as usize] > 0xFF {
                        self.v[idx as usize] &= 0x00FF;
                        self.v[0xF] = 1;
                    } else {
                        self.v[0xF] = 0;
                    }

                    self.pc += 2;
                }
                0x0005 => {
                    let idx = ((opcode & 0x0F00) >> 8) as usize;
                    let idy = ((opcode & 0x00F0) >> 4) as usize;

                    let vx = self.v[idx];
                    let vy = self.v[idy];

                    if vx >= vy {
                        self.v[0xF] = 1;
                    } else {
                        self.v[0xF] = 0;
                    }

                    self.v[idx] = vx.wrapping_sub(vy) & 0xFF;
                    self.pc += 2;
                }
                0x0006 => {
                    let idx = (opcode & 0x0F00) >> 8;

                    if self.v[idx as usize] & 0x1 == 0x1 {
                        self.v[0xF] = 1;
                    } else {
                        self.v[0xF] = 0;
                    }

                    self.v[idx as usize] /= 2;

                    self.pc += 2;
                }
                _ => {
                    println!(
                        "Line {}: Unknown instruction {:04X}",
                        self.pc - START_ADDR,
                        opcode
                    );

                    return Err(format!("Unimplemented instruction"));
                }
            },
            0xA000 => {
                let val = opcode & 0x0FFF;
                println!("Set I to {}", val);

                self.i = val;
                self.pc += 2;
            }
            0xD000 => {
                let x_pos = self.v[((opcode & 0x0F00) >> 8) as usize] % DISPLAY_WIDTH as u16;
                let y_pos = self.v[((opcode & 0x00F0) >> 4) as usize] % DISPLAY_HEIGHT as u16;
                let n = opcode & 0x000F;

                self.v[0xF] = 0;

                for row in 0..n {
                    let y = (y_pos + row) % DISPLAY_HEIGHT as u16;

                    let spr_byte = self.memory[(self.i + row) as usize];

                    for col in 0..8 {
                        let x = (x_pos + col) % DISPLAY_WIDTH as u16;

                        let sprite_pixel = (spr_byte >> (7 - col)) & 1;

                        if sprite_pixel == 1 {
                            let pixel_index = (y * DISPLAY_WIDTH as u16) + x;

                            if self.display[pixel_index as usize] == 1 {
                                self.v[0xF] = 1;
                            }

                            self.display[pixel_index as usize] ^= 1;
                        }
                    }
                }

                self.pc += 2;
            }
            0xE000 => match opcode & 0x00FF {
                0x009E => {
                    let idx = (opcode & 0x0F00) >> 8;

                    println!("Checking keys");
                    if self.keys[self.v[idx as usize] as usize] {
                        self.pc += 4;
                    } else {
                        self.pc += 2;
                    }
                }
                0x00A1 => {
                    let idx = (opcode & 0x0F00) >> 8;
                    println!("Checking keys");

                    if !self.keys[self.v[idx as usize] as usize] {
                        self.pc += 4;
                    } else {
                        self.pc += 2;
                    }
                }
                _ => {
                    println!(
                        "Line {}: Unknown instruction {:04X}",
                        self.pc - START_ADDR,
                        opcode
                    );

                    return Err(format!("Unimplemented instruction"));
                }
            },
            0xF000 => match opcode & 0x00FF {
                0x0007 => {
                    let idx = (opcode & 0x0F00) >> 8;

                    self.v[idx as usize] = self.dt;
                    self.pc += 2;
                }
                0x0015 => {
                    let idx = (opcode & 0x0F00) >> 8;

                    self.dt = self.v[idx as usize];
                    self.pc += 2;
                }
                0x0018 => {
                    let idx = (opcode & 0x0F00) >> 8;

                    self.st = self.v[idx as usize];
                    self.pc += 2;
                }
                0x001E => {
                    let val = self.v[((opcode & 0x0F00) >> 8) as usize];

                    self.i += val;
                    self.pc += 2;
                }
                0x0065 => {
                    let idx = (opcode & 0x0F00) >> 8;

                    for i in 0..=idx {
                        self.v[i as usize] = self.memory[(self.i + i) as usize] as u16;
                    }
                    self.pc += 2;
                }
                _ => {
                    println!(
                        "Line {}: Unknown instruction {:04X}",
                        self.pc - START_ADDR,
                        opcode
                    );

                    return Err(format!("Unimplemented instruction"));
                }
            },
            _ => {
                println!(
                    "Line {}: Unknown instruction {:04X}",
                    self.pc - START_ADDR,
                    opcode
                );

                return Err(format!("Unimplemented instruction"));
            }
        }
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window_attributes = Window::default_attributes()
                .with_title("winit 0.30 Window")
                .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0));

            let window = Arc::new(event_loop.create_window(window_attributes).unwrap());

            self.window = Some(window.clone());

            let size = window.inner_size();
            let st = SurfaceTexture::new(size.width, size.height, window.clone());

            let pixels = Pixels::new(DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32, st).unwrap();

            self.pixels = Some(pixels);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                println!("Close button was pressed stopping");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(pixels) = &mut self.pixels {
                    if let Err(err) = pixels.resize_surface(size.width, size.height) {
                        println!("Resize error: {}", err);
                        event_loop.exit();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(pixels) = &mut self.pixels {
                    let frame = pixels.frame_mut();

                    for (i, pixel) in frame.chunks_exact_mut(4).enumerate() {
                        let is_pixel_on = self.display[i] != 0;

                        let rgba = if is_pixel_on {
                            [0x00, 0xFF, 0x00, 0xFF]
                        } else {
                            [0x00, 0x00, 0x00, 0xFF]
                        };

                        pixel.copy_from_slice(&rgba);
                    }

                    if let Err(err) = pixels.render() {
                        println!("Render error: {}", err);
                        event_loop.exit();
                    }
                }
            }
            WindowEvent::KeyboardInput {
                device_id,
                event,
                is_synthetic,
            } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let is_pressed = event.state == ElementState::Pressed;

                    match code {
                        KeyCode::Digit1 => self.keys[0x1] = is_pressed,
                        KeyCode::Digit2 => self.keys[0x2] = is_pressed,
                        KeyCode::Digit3 => self.keys[0x3] = is_pressed,
                        KeyCode::Digit4 => self.keys[0xC] = is_pressed,

                        KeyCode::KeyQ => self.keys[0x4] = is_pressed,
                        KeyCode::KeyW => self.keys[0x5] = is_pressed,
                        KeyCode::KeyE => self.keys[0x6] = is_pressed,
                        KeyCode::KeyR => self.keys[0xD] = is_pressed,

                        KeyCode::KeyA => self.keys[0x7] = is_pressed,
                        KeyCode::KeyS => self.keys[0x8] = is_pressed,
                        KeyCode::KeyD => self.keys[0x9] = is_pressed,

                        KeyCode::KeyZ => self.keys[0xA] = is_pressed,
                        KeyCode::KeyX => self.keys[0x0] = is_pressed,
                        KeyCode::KeyC => self.keys[0xB] = is_pressed,
                        KeyCode::KeyV => self.keys[0xF] = is_pressed,

                        KeyCode::Escape => {
                            println!("Escape pressed");
                            event_loop.exit();
                        }

                        _ => (),
                    }
                }
            }
            _ => (),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.crashed {
            if let Err(e) = self.step_cpu() {
                println!("CPU Error: {}", e);
                self.crashed = true;
            }

            let now = Instant::now();

            if now.duration_since(self.last_timer_update).as_secs_f32() >= (1.0 / 60.0) {
                if self.dt > 0 {
                    self.dt -= 1;
                }
                if self.st > 0 {
                    self.st -= 1;
                }

                self.last_timer_update = now;
            }
        }

        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // We MUST drop the GPU surface before the window,
        // and both before the Wayland connection dies!
        self.pixels = None;
        self.window = None;
        println!("Safely tore down graphics resources!");
    }
}

fn fetch_opcode(memory: &[u8; MEM_LEN], pc: usize) -> Result<u16, String> {
    if pc + 1 >= MEM_LEN {
        return Err(format!("PC out of bounds: {pc:#x}"));
    }

    Ok(((memory[pc] as u16) << 8) | (memory[pc + 1] as u16))
}

fn load_binary(memory: &mut [u8; 4096], path: &str) {
    if let Ok(v) = std::fs::read(path) {
        memory[START_ADDR..(v.len() + START_ADDR)].clone_from_slice(&v);
    } else {
        panic!("Couldn't read file.");
    }
}

fn main() -> Result<(), String> {
    println!("Full start");
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new();

    event_loop.run_app(&mut app).unwrap();
    Ok(())
}
