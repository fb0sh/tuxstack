//! PROTOTYPE — throwaway. Answers: what should tuxstack's GUI look like?
//!
//! Shows 3 layout variants side-by-side, switchable via keyboard.

use slint::{ModelRc, VecModel};
use std::rc::Rc;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    // Populate with mock data
    let containers = vec![
        ContainerData {
            name: "web-app".into(),
            image: "nginx:latest".into(),
            status: "running".into(),
            cpu: "2.3%".into(),
            memory: "43.5 MB".into(),
            ports: "8080→80".into(),
        },
        ContainerData {
            name: "redis-cache".into(),
            image: "redis:7-alpine".into(),
            status: "running".into(),
            cpu: "0.5%".into(),
            memory: "11.8 MB".into(),
            ports: "6379→6379".into(),
        },
        ContainerData {
            name: "postgres-db".into(),
            image: "postgres:16".into(),
            status: "exited".into(),
            cpu: "-".into(),
            memory: "-".into(),
            ports: "5432→5432".into(),
        },
        ContainerData {
            name: "api-server".into(),
            image: "node:22".into(),
            status: "running".into(),
            cpu: "5.1%".into(),
            memory: "128.2 MB".into(),
            ports: "3000→3000".into(),
        },
    ];

    let model = Rc::new(VecModel::from(containers));
    let model_clone = model.clone();
    ui.global::<AppState>().set_containers(ModelRc::from(model_clone));

    ui.run()
}
