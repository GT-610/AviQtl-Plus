slint::slint! {
    export component AppWindow inherits Window {
        width: 480px;
        height: 320px;
        title: "Slint software probe";

        in-out property <bool> done;

        VerticalLayout {
            alignment: center;
            padding: 24px;
            spacing: 12px;

            Text {
                text: "software renderer";
                font-size: 24px;
                horizontal-alignment: center;
            }
            Text {
                text: "no GPU required";
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
    let app = AppWindow::new()?;
    let weak = app.as_weak();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(6));
        let app = weak.upgrade().unwrap();
        if app.get_done() {
            println!("PROBE_RESULT clicked=true");
        } else {
            println!("PROBE_RESULT clicked=false");
        }
        app.hide().unwrap();
    });
    app.run()?;
    Ok(())
}
