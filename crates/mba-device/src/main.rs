use std::{
    convert::Infallible,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use clap::Parser;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{ascii::FONT_10X20, ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::{raw::RawU16, Rgb565},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
    Pixel,
};
use mba_protocol::NetworkMode;
use rppal::{
    gpio::{Gpio, InputPin, Level, OutputPin},
    spi::{Bus, Mode, SlaveSelect, Spi},
};
use tracing::{error, info, warn};

const DISPLAY_WIDTH: u32 = 240;
const DISPLAY_HEIGHT: u32 = 240;
const BUTTON_A_PIN: u8 = 5;
const BUTTON_B_PIN: u8 = 6;
const BUTTON_X_PIN: u8 = 16;
const DEFAULT_Y_BUTTON_PINS: &[u8] = &[20, 24];
const LONG_PRESS: Duration = Duration::from_millis(1200);
const SHORT_PRESS_MIN: Duration = Duration::from_millis(80);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const DISPLAY_REFRESH: Duration = Duration::from_secs(2);
const BUTTON_COOLDOWN: Duration = Duration::from_secs(2);
const SPI_WRITE_CHUNK: usize = 4096;

const DISPLAY_DC_PIN: u8 = 9;
const DISPLAY_BACKLIGHT_PIN: u8 = 13;
const DISPLAY_SPI_CLOCK_HZ: u32 = 62_500_000;

// ST7789 controller commands used by the Pirate Audio display.
const ST7789_SWRESET: u8 = 0x01;
const ST7789_SLPOUT: u8 = 0x11;
const ST7789_NORON: u8 = 0x13;
const ST7789_INVON: u8 = 0x21;
const ST7789_CASET: u8 = 0x2A;
const ST7789_RASET: u8 = 0x2B;
const ST7789_RAMWR: u8 = 0x2C;
const ST7789_MADCTL: u8 = 0x36;
const ST7789_COLMOD: u8 = 0x3A;
const ST7789_DISPON: u8 = 0x29;

// Pixel format = 16bpp RGB565.
const ST7789_COLMOD_RGB565: u8 = 0x55;
// Memory data access: row/column exchange + RGB order for the Pirate Audio panel.
const ST7789_MADCTL_PIRATE: u8 = 0x70;

#[derive(Debug, Parser)]
#[command(author, version, about = "Matchbox Audio device display and buttons")]
struct Args {
    /// Path to the network-mode helper script.
    #[arg(long, default_value = "/usr/bin/mba-network-mode")]
    network_script: String,

    /// BCM GPIO pins to treat as the fourth Pirate Audio button.
    #[arg(long = "fourth-button-gpio")]
    fourth_button_gpios: Vec<u8>,

    /// Disable display initialization and only monitor buttons.
    #[arg(long)]
    no_display: bool,
}

fn main() -> Result<()> {
    init_logging();

    let args = Args::parse();
    let y_button_pins = if args.fourth_button_gpios.is_empty() {
        DEFAULT_Y_BUTTON_PINS.to_vec()
    } else {
        args.fourth_button_gpios
    };
    let button_specs = vec![
        ButtonSpec::new("A", vec![BUTTON_A_PIN], ButtonAction::LogOnly),
        ButtonSpec::new("B", vec![BUTTON_B_PIN], ButtonAction::LogOnly),
        ButtonSpec::new("X", vec![BUTTON_X_PIN], ButtonAction::LogOnly),
        ButtonSpec::new("Y", y_button_pins, ButtonAction::NetworkToggle),
    ];
    let button_summary = button_specs
        .iter()
        .map(|spec| format!("{}:{:?}", spec.name, spec.gpios))
        .collect::<Vec<_>>();

    info!(?button_summary, "starting mba-device");

    let mut display = if args.no_display {
        None
    } else {
        match PirateDisplay::open() {
            Ok(display) => Some(display),
            Err(error) => {
                warn!(%error, "display initialization failed; continuing with buttons only");
                None
            }
        }
    };

    let mut buttons = match ButtonSet::open(&button_specs) {
        Ok(buttons) => Some(buttons),
        Err(error) => {
            warn!(%error, "button initialization failed; display will still run");
            None
        }
    };

    let mut frame = Framebuffer::new(Rgb565::BLACK);
    let mut last_display = expired(DISPLAY_REFRESH);
    let mut last_toggle = expired(BUTTON_COOLDOWN);
    let mut message = String::from("Hold Y for network");

    loop {
        if let Some(buttons) = buttons.as_mut() {
            for event in buttons.poll() {
                log_button_event(&event);
                match (event.action, event.kind) {
                    (ButtonAction::NetworkToggle, ButtonEventKind::ShortPress) => {
                        message = String::from("Hold Y to switch");
                        last_display = expired(DISPLAY_REFRESH);
                    }
                    (ButtonAction::NetworkToggle, ButtonEventKind::LongPress) => {
                        if last_toggle.elapsed() >= BUTTON_COOLDOWN {
                            message = String::from("Switching network...");
                            render_status(
                                display.as_mut(),
                                &mut frame,
                                &args.network_script,
                                &message,
                            );
                            match run_network_toggle(&args.network_script) {
                                Ok(output) => {
                                    let mode = NetworkMode::parse(
                                        parse_field(&output, "mode").unwrap_or("unknown"),
                                    );
                                    message = format!("Network: {mode}");
                                    info!(%mode, "network mode toggled");
                                }
                                Err(error) => {
                                    message = String::from("Network switch failed");
                                    error!(%error, "network mode toggle failed");
                                }
                            }
                            last_toggle = Instant::now();
                            last_display = expired(DISPLAY_REFRESH);
                        }
                    }
                    (ButtonAction::LogOnly, _) => {}
                }
            }
        }

        if last_display.elapsed() >= DISPLAY_REFRESH {
            render_status(display.as_mut(), &mut frame, &args.network_script, &message);
            last_display = Instant::now();
        }

        thread::sleep(POLL_INTERVAL);
    }
}

// Returns an Instant far enough in the past that elapsed() is at least `duration`,
// without panicking on early boot when the monotonic clock is younger than `duration`.
fn expired(duration: Duration) -> Instant {
    Instant::now()
        .checked_sub(duration)
        .unwrap_or_else(Instant::now)
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mba_device=info,warn".into()),
        )
        .init();
}

fn render_status(
    display: Option<&mut PirateDisplay>,
    frame: &mut Framebuffer,
    network_script: &str,
    message: &str,
) {
    let Some(display) = display else {
        return;
    };

    let status = match network_status(network_script) {
        Ok(status) => status,
        Err(error) => {
            warn!(%error, "failed to read network status");
            NetworkStatus {
                mode: NetworkMode::Unknown,
                active_connection: String::from("-"),
                ip4: String::from("-"),
                hotspot_ssid: String::from("matchbox-audio"),
                hotspot_password: String::from("-"),
            }
        }
    };

    draw_dashboard(frame, &status, message);

    if let Err(error) = display.flush(frame) {
        warn!(%error, "display refresh failed");
    }
}

fn draw_dashboard(frame: &mut Framebuffer, status: &NetworkStatus, message: &str) {
    let title = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let label = MonoTextStyle::new(&FONT_6X10, Rgb565::new(16, 38, 22));
    let body = MonoTextStyle::new(&FONT_10X20, Rgb565::CYAN);
    let muted = MonoTextStyle::new(&FONT_6X10, Rgb565::new(18, 30, 18));
    let accent = match status.mode {
        NetworkMode::Car => Rgb565::YELLOW,
        _ => Rgb565::GREEN,
    };

    let _ = Rectangle::new(Point::new(0, 0), Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
        .draw(frame);
    let _ = Rectangle::new(Point::new(0, 0), Size::new(DISPLAY_WIDTH, 30))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::new(0, 16, 18)))
        .draw(frame);
    let _ = Text::new("Matchbox Audio", Point::new(8, 22), title).draw(frame);

    let mode = status.mode.as_str().to_uppercase();
    let _ = Text::new("network", Point::new(8, 48), label).draw(frame);
    let _ = Text::new(
        &mode,
        Point::new(8, 72),
        MonoTextStyle::new(&FONT_10X20, accent),
    )
    .draw(frame);

    let _ = Text::new("connection", Point::new(8, 96), label).draw(frame);
    let _ = Text::new(
        truncate(&status.active_connection, 20),
        Point::new(8, 120),
        body,
    )
    .draw(frame);

    let _ = Text::new("address", Point::new(8, 144), label).draw(frame);
    let _ = Text::new(truncate(&status.ip4, 20), Point::new(8, 168), body).draw(frame);

    if status.mode == NetworkMode::Car {
        let _ = Text::new("hotspot", Point::new(8, 192), label).draw(frame);
        let _ = Text::new(
            truncate(&status.hotspot_ssid, 20),
            Point::new(8, 214),
            muted,
        )
        .draw(frame);
        let pass = format!("pass {}", status.hotspot_password);
        let _ = Text::new(truncate(&pass, 28), Point::new(8, 230), muted).draw(frame);
    } else {
        let _ = Text::new(truncate(message, 28), Point::new(8, 218), muted).draw(frame);
    }
}

fn truncate(value: &str, max_chars: usize) -> &str {
    if value.chars().count() <= max_chars {
        return value;
    }

    let mut end = value.len();
    for (count, (index, _)) in value.char_indices().enumerate() {
        if count == max_chars {
            end = index;
            break;
        }
    }
    &value[..end]
}

#[derive(Debug)]
struct NetworkStatus {
    mode: NetworkMode,
    active_connection: String,
    ip4: String,
    hotspot_ssid: String,
    hotspot_password: String,
}

fn network_status(network_script: &str) -> Result<NetworkStatus> {
    let output = run_network_command(network_script, "status")?;
    Ok(NetworkStatus {
        mode: NetworkMode::parse(parse_field(&output, "mode").unwrap_or("unknown")),
        active_connection: parse_field(&output, "active_connection")
            .or_else(|| parse_field(&output, "ssid"))
            .unwrap_or("-")
            .to_string(),
        ip4: parse_field(&output, "ip4").unwrap_or("-").to_string(),
        hotspot_ssid: parse_field(&output, "hotspot_ssid")
            .unwrap_or("matchbox-audio")
            .to_string(),
        hotspot_password: parse_field(&output, "hotspot_password")
            .unwrap_or("-")
            .to_string(),
    })
}

fn run_network_toggle(network_script: &str) -> Result<String> {
    run_network_command(network_script, "toggle")
}

fn run_network_command(network_script: &str, command: &str) -> Result<String> {
    let output = Command::new(network_script)
        .arg(command)
        .output()
        .with_context(|| format!("failed to run {network_script} {command}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{network_script} {command} failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_field<'a>(output: &'a str, field: &str) -> Option<&'a str> {
    let prefix = format!("{field}=");
    output
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonAction {
    LogOnly,
    NetworkToggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonEventKind {
    ShortPress,
    LongPress,
}

#[derive(Debug, Clone)]
struct ButtonSpec {
    name: &'static str,
    gpios: Vec<u8>,
    action: ButtonAction,
}

impl ButtonSpec {
    fn new(name: &'static str, gpios: Vec<u8>, action: ButtonAction) -> Self {
        Self {
            name,
            gpios,
            action,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ButtonEvent {
    name: &'static str,
    gpio: u8,
    action: ButtonAction,
    kind: ButtonEventKind,
    elapsed: Duration,
}

fn log_button_event(event: &ButtonEvent) {
    let duration_ms = event.elapsed.as_millis();
    match event.kind {
        ButtonEventKind::ShortPress => {
            info!(
                button = event.name,
                gpio = event.gpio,
                duration_ms,
                action = ?event.action,
                "button short press"
            );
        }
        ButtonEventKind::LongPress => {
            info!(
                button = event.name,
                gpio = event.gpio,
                duration_ms,
                action = ?event.action,
                "button long press"
            );
        }
    }
}

struct ButtonSet {
    pins: Vec<ButtonState>,
}

impl ButtonSet {
    fn open(specs: &[ButtonSpec]) -> Result<Self> {
        let gpio = Gpio::new().context("failed to open GPIO")?;
        let mut pins = Vec::new();
        for spec in specs {
            for &number in &spec.gpios {
                let pin = gpio
                    .get(number)
                    .with_context(|| format!("failed to open GPIO {number}"))?
                    .into_input_pullup();
                pins.push(ButtonState {
                    name: spec.name,
                    number,
                    action: spec.action,
                    pin,
                    pressed_since: None,
                    was_pressed: false,
                });
            }
        }
        Ok(Self { pins })
    }

    fn poll(&mut self) -> Vec<ButtonEvent> {
        let mut events = Vec::new();
        for state in &mut self.pins {
            if let Some(event) = state.poll() {
                events.push(event);
            }
        }
        events
    }
}

struct ButtonState {
    name: &'static str,
    number: u8,
    action: ButtonAction,
    pin: InputPin,
    pressed_since: Option<Instant>,
    was_pressed: bool,
}

impl ButtonState {
    fn poll(&mut self) -> Option<ButtonEvent> {
        let pressed = self.pin.read() == Level::Low;
        let now = Instant::now();

        if pressed && !self.was_pressed {
            self.pressed_since = Some(now);
        }

        if !pressed && self.was_pressed {
            let elapsed = self
                .pressed_since
                .map(|started| now.saturating_duration_since(started))
                .unwrap_or_default();
            self.pressed_since = None;
            self.was_pressed = false;
            if elapsed >= LONG_PRESS {
                return Some(ButtonEvent {
                    name: self.name,
                    gpio: self.number,
                    action: self.action,
                    kind: ButtonEventKind::LongPress,
                    elapsed,
                });
            }
            if elapsed >= SHORT_PRESS_MIN {
                return Some(ButtonEvent {
                    name: self.name,
                    gpio: self.number,
                    action: self.action,
                    kind: ButtonEventKind::ShortPress,
                    elapsed,
                });
            }
            return None;
        }

        self.was_pressed = pressed;
        None
    }
}

struct PirateDisplay {
    spi: Spi,
    dc: OutputPin,
    backlight: OutputPin,
}

impl PirateDisplay {
    fn open() -> Result<Self> {
        let gpio = Gpio::new().context("failed to open GPIO")?;
        let dc = gpio
            .get(DISPLAY_DC_PIN)
            .with_context(|| format!("failed to open GPIO {DISPLAY_DC_PIN} for display DC"))?
            .into_output();
        let mut backlight = gpio
            .get(DISPLAY_BACKLIGHT_PIN)
            .with_context(|| {
                format!("failed to open GPIO {DISPLAY_BACKLIGHT_PIN} for display backlight")
            })?
            .into_output();
        backlight.set_high();

        let spi = Spi::new(
            Bus::Spi0,
            SlaveSelect::Ss1,
            DISPLAY_SPI_CLOCK_HZ,
            Mode::Mode0,
        )
        .context("failed to open SPI0 CE1 for Pirate Audio display")?;

        let mut display = Self { spi, dc, backlight };
        display.init()?;
        Ok(display)
    }

    fn init(&mut self) -> Result<()> {
        self.command(ST7789_SWRESET)?;
        thread::sleep(Duration::from_millis(150));
        self.command(ST7789_SLPOUT)?;
        thread::sleep(Duration::from_millis(120));
        self.command_data(ST7789_COLMOD, &[ST7789_COLMOD_RGB565])?;
        self.command_data(ST7789_MADCTL, &[ST7789_MADCTL_PIRATE])?;
        self.command(ST7789_INVON)?;
        self.command(ST7789_NORON)?;
        self.command(ST7789_DISPON)?;
        thread::sleep(Duration::from_millis(20));
        Ok(())
    }

    fn flush(&mut self, frame: &mut Framebuffer) -> Result<()> {
        self.backlight.set_high();
        self.set_window(0, 0, DISPLAY_WIDTH as u16 - 1, DISPLAY_HEIGHT as u16 - 1)?;
        self.command(ST7789_RAMWR)?;
        self.dc.set_high();
        self.write_spi(frame.as_bytes(), "failed to write display frame")?;
        Ok(())
    }

    fn set_window(&mut self, x0: u16, y0: u16, x1: u16, y1: u16) -> Result<()> {
        self.command_data(ST7789_CASET, &[hi(x0), lo(x0), hi(x1), lo(x1)])?;
        self.command_data(ST7789_RASET, &[hi(y0), lo(y0), hi(y1), lo(y1)])?;
        Ok(())
    }

    fn command(&mut self, command: u8) -> Result<()> {
        self.dc.set_low();
        self.write_spi(
            &[command],
            &format!("failed to write display command 0x{command:02x}"),
        )?;
        Ok(())
    }

    fn command_data(&mut self, command: u8, data: &[u8]) -> Result<()> {
        self.command(command)?;
        self.dc.set_high();
        self.write_spi(
            data,
            &format!("failed to write display command data 0x{command:02x}"),
        )?;
        Ok(())
    }

    fn write_spi(&mut self, data: &[u8], context: &str) -> Result<()> {
        for chunk in data.chunks(SPI_WRITE_CHUNK) {
            let written = self.spi.write(chunk).with_context(|| context.to_string())?;
            if written != chunk.len() {
                bail!("{context}: short SPI write ({written}/{})", chunk.len());
            }
        }
        Ok(())
    }
}

fn hi(value: u16) -> u8 {
    (value >> 8) as u8
}

fn lo(value: u16) -> u8 {
    value as u8
}

struct Framebuffer {
    pixels: Vec<Rgb565>,
    bytes: Vec<u8>,
}

impl Framebuffer {
    fn new(color: Rgb565) -> Self {
        let count = (DISPLAY_WIDTH * DISPLAY_HEIGHT) as usize;
        Self {
            pixels: vec![color; count],
            bytes: Vec::with_capacity(count * 2),
        }
    }

    fn as_bytes(&mut self) -> &[u8] {
        self.bytes.clear();
        for pixel in &self.pixels {
            let raw = RawU16::from(*pixel).into_inner();
            self.bytes.push((raw >> 8) as u8);
            self.bytes.push(raw as u8);
        }
        &self.bytes
    }
}

impl OriginDimensions for Framebuffer {
    fn size(&self) -> Size {
        Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT)
    }
}

impl DrawTarget for Framebuffer {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x < 0 || point.y < 0 {
                continue;
            }
            let x = point.x as u32;
            let y = point.y as u32;
            if x >= DISPLAY_WIDTH || y >= DISPLAY_HEIGHT {
                continue;
            }
            let index = (y * DISPLAY_WIDTH + x) as usize;
            self.pixels[index] = color;
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.pixels.fill(color);
        Ok(())
    }
}
