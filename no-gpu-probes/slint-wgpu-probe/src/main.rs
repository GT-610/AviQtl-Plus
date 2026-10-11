use slint::ComponentHandle;
use slint::wgpu_29::wgpu;

slint::slint! {
    export component AppWindow inherits Window {
        width: 480px;
        height: 320px;
        title: "Slint wgpu probe";

        in-out property <bool> done;

        VerticalLayout {
            alignment: center;
            padding: 24px;
            spacing: 12px;

            Text {
                text: "wgpu renderer";
                font-size: 24px;
                horizontal-alignment: center;
            }
            TouchArea {
                width: 120px;
                height: 40px;
                Rectangle {
                    background: #3a6ea5;
                    border-radius: 6px;
                    Text {
                        text: "Close";
                        color: white;
                        horizontal-alignment: center;
                        vertical-alignment: center;
                    }
                }
                clicked => { root.done = true; }
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let force_cpu = std::env::args().any(|arg| arg == "--force-cpu");
    let instance = wgpu::Instance::default();
    let adapter = if force_cpu {
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: true,
            compatible_surface: None,
        }))?
    } else {
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))?
    };
    let info = adapter.get_info();
    eprintln!("PROBE adapter: {} ({:?}) type={:?}", info.name, info.backend, info.device_type);
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("probe-device"),
            ..Default::default()
        },
    ))?;

    if std::env::args().any(|arg| arg == "--slint-cpu") {
        // Slint's selector refuses wgpu 29 without a GPU-backed adapter unless this is set.
        // Reaching the env from a child process is done via the launcher script.
    }

    let mut settings = slint::wgpu_29::WGPUSettings::default();
    settings.power_preference = wgpu::PowerPreference::LowPower;
    slint::BackendSelector::new()
        .require_wgpu_29(slint::wgpu_29::WGPUConfiguration::Automatic(settings))
        .select()?;

    let app = AppWindow::new()?;
    let weak = app.as_weak();
    let tick = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let tick_timer = tick.clone();
    let timer = slint::Timer::default();
    let tick_timer = tick.clone();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(16),
        move || {
            tick_timer.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        },
    );
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(8));
        let app = weak.upgrade().unwrap();
        let ticks = tick.load(std::sync::atomic::Ordering::SeqCst);
        println!("PROBE_RESULT ticks={ticks} done={}", app.get_done());
        std::process::exit(0);
    });
    app.run()?;
    Ok(())
}
