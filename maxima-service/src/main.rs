#[macro_use]
extern crate windows_service;

use std::fs::File;

use actix_web::{get, post, web, HttpResponse, Responder};
use anyhow::Result;
use dll_syringe::process::OwnedProcess;
use dll_syringe::Syringe;
use maxima::util::registry::set_up_registry;
use std::ffi::OsString;
use std::path::Path;
use std::thread;
use std::time::Duration;
use structured_logger::json::new_writer;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::{
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

use maxima::core::background_service::{ServiceLibraryInjectionRequest, BACKGROUND_SERVICE_PORT};

define_windows_service!(ffi_service_main, my_service_main);

fn my_service_main(arguments: Vec<OsString>) {
    if let Err(_e) = bootstrap_service(arguments) {
        // Handle error in some way.
    }
}

fn bootstrap_service(arguments: Vec<OsString>) -> windows_service::Result<()> {
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop | ServiceControl::Interrogate => {
                std::process::exit(0);
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    // Register system service event handler
    let status_handle =
        service_control_handler::register("MaximaBackgroundService", event_handler)?;

    let next_status = ServiceStatus {
        // Should match the one from system service registry
        service_type: ServiceType::OWN_PROCESS,
        // The new state
        current_state: ServiceState::Running,
        // Accept stop events when running
        controls_accepted: ServiceControlAccept::STOP,
        // Used to report an error when starting or stopping only, otherwise must be zero
        exit_code: ServiceExitCode::Win32(0),
        // Only used for pending states, otherwise must be zero
        checkpoint: 0,
        // Only used for pending states, otherwise must be zero
        wait_hint: Duration::default(),
        // Unused for setting status
        process_id: None,
    };

    // Tell the system that the service is running now
    status_handle.set_service_status(next_status)?;

    // Do some work
    run_service().expect("Failed to run service");

    Ok(())
}

#[get("/set_up_registry")]
async fn req_set_up_registry() -> impl Responder {
    log::info!("Setting up registry");
    let result = set_up_registry();
    if result.is_err() {
        return format!("Error: {}", result.err().unwrap());
    }

    format!("Hello!")
}

#[post("/inject_library")]
async fn req_inject_library(body: web::Bytes) -> Result<HttpResponse, actix_web::Error> {
    log::info!("Injecting...");
    let obj: ServiceLibraryInjectionRequest = serde_json::from_slice(&body)?;
    let process = OwnedProcess::from_pid(obj.pid)?;
    let syringe = Syringe::for_process(process);
    syringe.inject(obj.path).unwrap();

    Ok(HttpResponse::Ok().body("Injected"))
}

fn run_service() -> Result<()> {
    let log_path = Path::new("C:/ProgramData/Maxima/Logs/MaximaBackgroundService.log");
    std::fs::create_dir_all(log_path.parent().unwrap())?;
    let log_file = File::create(log_path)?;

    structured_logger::Builder::new()
        .with_default_writer(new_writer(log_file))
        .init();

    log::info!("Started Background Service");

    let server_thread = thread::spawn(|| {
        actix_web::rt::System::new().block_on(async {
            // Import your actix_web::App here
            use actix_web::{web, App, HttpResponse, HttpServer, Responder};

            HttpServer::new(|| {
                App::new()
                    .service(req_set_up_registry)
                    .service(req_inject_library)
            })
            .bind(("127.0.0.1", BACKGROUND_SERVICE_PORT))
            .unwrap()
            .run()
            .await
            .unwrap();
        });
    });

    Ok(())
}

fn main() -> Result<()> {
    // Register generated `ffi_service_main` with the system and start the service, blocking
    // this thread until the service is stopped.
    service_dispatcher::start("MaximaBackgroundService", ffi_service_main)?;
    Ok(())
}
