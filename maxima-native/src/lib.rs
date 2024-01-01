use core::slice;
use std::{
    ffi::{c_ushort, CStr, CString},
    os::raw::{c_char, c_uint, c_void},
    sync::Arc,
};

use anyhow::{bail, Error, Result};

use maxima::{
    core::{
        auth::{context::AuthContext, login},
        launch, Maxima, MaximaEvent,
    },
    util::{
        log::init_logger,
        native::take_foreground_focus,
        registry::{check_registry_validity, read_game_path},
    },
};

#[cfg(windows)]
use maxima::{
    core::background_service::request_registry_setup,
    util::service::{
        is_service_running, is_service_valid, register_service_user, start_service, stop_service,
    },
};

use maxima::core::auth::execute_connect_token;
use tokio::{runtime::Runtime, sync::Mutex};

pub const ERR_SUCCESS: usize = 0;
pub const ERR_UNKNOWN: usize = 1;
pub const ERR_CHECK_LE: usize = 2;
pub const ERR_LOGIN_FAILED: usize = 3;
pub const ERR_INVALID_ARGUMENT: usize = 4;

static mut LAST_ERROR: Option<String> = None;

/// Get the last error.
#[no_mangle]
pub unsafe extern "C" fn maxima_get_last_error() -> *const c_char {
    if LAST_ERROR.is_none() {
        return std::ptr::null();
    }

    CString::new(LAST_ERROR.to_owned().unwrap())
        .unwrap()
        .into_raw()
}

fn set_last_error(err: Error) {
    unsafe { LAST_ERROR = Some(err.to_string()) };
}

fn set_last_error_from_result<T>(result: Result<T>) {
    set_last_error(result.err().unwrap());
}

/// Set up Maxima's logging.
#[no_mangle]
pub extern "C" fn maxima_init_logger() -> usize {
    init_logger();
    ERR_SUCCESS
}

/// Create an asynchronous runtime.
#[no_mangle]
pub extern "C" fn maxima_create_runtime(runtime_out: *mut *mut c_void) -> usize {
    let result = Runtime::new();
    if result.is_err() {
        set_last_error(result.err().unwrap().into());
        return ERR_CHECK_LE;
    }

    let runtime = Box::new(result.unwrap());
    unsafe { *runtime_out = Box::into_raw(runtime) as *mut c_void }

    ERR_SUCCESS
}

/// Check if the Maxima Background Service is installed and valid.
#[no_mangle]
#[cfg(windows)]
pub extern "C" fn maxima_is_service_valid(out: *mut bool) -> usize {
    let result = is_service_valid();
    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    unsafe { *out = result.unwrap() };
    ERR_SUCCESS
}

/// Check if the Maxima Background Service is running.
#[no_mangle]
#[cfg(windows)]
pub extern "C" fn maxima_is_service_running(out: *mut bool) -> usize {
    let result = is_service_running();
    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    unsafe { *out = result.unwrap() };
    ERR_SUCCESS
}

/// Register the Maxima Background Service. Runs maxima-bootstrap for admin access.
#[no_mangle]
#[cfg(windows)]
pub extern "C" fn maxima_register_service() -> usize {
    let result = register_service_user();
    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    ERR_SUCCESS
}

/// Start the Maxima Background Service.
#[no_mangle]
#[cfg(windows)]
pub extern "C" fn maxima_start_service(runtime: *mut *mut Runtime) -> usize {
    let rt = unsafe { Box::from_raw(*runtime) };

    let result = rt.block_on(async { start_service().await });
    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    unsafe {
        *runtime = Box::into_raw(rt);
    }

    ERR_SUCCESS
}

/// Stop the Maxima Background Service.
#[no_mangle]
#[cfg(windows)]
pub extern "C" fn maxima_stop_service(runtime: *mut *mut Runtime) -> usize {
    let rt = unsafe { Box::from_raw(*runtime) };

    let result = rt.block_on(async { stop_service().await });
    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    unsafe {
        *runtime = Box::into_raw(rt);
    }

    ERR_SUCCESS
}

/// Check if the Windows Registry is properly set up for Maxima.
#[no_mangle]
pub extern "C" fn maxima_check_registry_validity() -> bool {
    check_registry_validity().is_ok()
}

/// Request the Maxima Background Service to set up the Windows Registry.
#[no_mangle]
#[cfg(windows)]
pub extern "C" fn maxima_request_registry_setup(runtime: *mut *mut Runtime) -> usize {
    let rt = unsafe { Box::from_raw(*runtime) };
    let result = rt.block_on(async { request_registry_setup().await });

    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    unsafe { *runtime = Box::into_raw(rt) }

    ERR_SUCCESS
}

/// Log into an EA account and retrieve an access token. Opens the EA website for authentication.
#[no_mangle]
pub extern "C" fn maxima_login(runtime: *mut *mut Runtime, token_out: *mut *mut c_char) -> usize {
    let rt = unsafe { Box::from_raw(*runtime) };

    let auth_context = AuthContext::new();
    if auth_context.is_err() {
        set_last_error_from_result(auth_context);
        return ERR_CHECK_LE;
    }

    let mut auth_context = auth_context.unwrap();

    let result = rt.block_on(async { login::begin_oauth_login_flow(&mut auth_context).await });
    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    let token = rt.block_on(async { execute_connect_token(&auth_context).await });
    if token.is_err() {
        set_last_error_from_result(token);
        return ERR_LOGIN_FAILED;
    }

    let raw_token = CString::new(token.unwrap().access_token().to_owned());
    if raw_token.is_err() {
        set_last_error(raw_token.err().unwrap().into());
        return ERR_CHECK_LE;
    }

    unsafe {
        *runtime = Box::into_raw(rt);
        *token_out = raw_token.unwrap().into_raw();
    }

    ERR_SUCCESS
}

/// Create a Maxima object.
#[no_mangle]
pub extern "C" fn maxima_mx_create() -> *const c_void {
    let maxima_arc = Maxima::new().expect("Failed to initialize Maxima");
    Arc::into_raw(maxima_arc) as *const c_void
}

/// Set the stored token retrieved from [maxima_login].
#[no_mangle]
pub extern "C" fn maxima_mx_set_access_token(
    runtime: *mut *mut Runtime,
    mx: *mut *const c_void,
    token: *const c_char,
) -> usize {
    if mx.is_null() || token.is_null() {
        return ERR_INVALID_ARGUMENT;
    }

    unsafe {
        let maxima_arc = Arc::from_raw(*mx as *const Mutex<Maxima>);

        let rt = Box::from_raw(*runtime);
        rt.block_on(async {
            let str_buf = parse_raw_string(token);
            //maxima_arc.lock().await.set_access_token(str_buf);
            todo!(); // I need to deal with this later, I have too much of a headache right now - BattleDash
        });

        *runtime = Box::into_raw(rt);
        *mx = Arc::into_raw(maxima_arc) as *const c_void;
    }

    ERR_SUCCESS
}

/// Set the port to be used for the LSX server. This will be automatically passed to games.
/// Note that not every game supports a custom LSX port, the default is 3216.
#[no_mangle]
pub unsafe extern "C" fn maxima_mx_set_lsx_port(
    runtime: *mut *mut Runtime,
    mx: *mut *const c_void,
    port: c_ushort,
) {
    let maxima_arc = Arc::from_raw(*mx as *const Mutex<Maxima>);

    let rt = Box::from_raw(*runtime);
    rt.block_on(async {
        maxima_arc.lock().await.set_lsx_port(port);
    });

    *runtime = Box::into_raw(rt);
    *mx = Arc::into_raw(maxima_arc) as *const c_void;
}

/// Start the LSX server used for game communication.
#[no_mangle]
pub extern "C" fn maxima_mx_start_lsx(runtime: *mut *mut Runtime, mx: *mut *const c_void) -> usize {
    if runtime.is_null() || mx.is_null() {
        return ERR_INVALID_ARGUMENT;
    }

    let result = unsafe {
        let maxima_arc = Arc::from_raw(*mx as *const Mutex<Maxima>);

        let rt = Box::from_raw(*runtime);
        let result =
            rt.block_on(async { maxima_arc.lock().await.start_lsx(maxima_arc.clone()).await });

        *runtime = Box::into_raw(rt);
        *mx = Arc::into_raw(maxima_arc) as *const c_void;
        result
    };

    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    ERR_SUCCESS
}

/// Consume pending LSX events.
#[no_mangle]
pub extern "C" fn maxima_mx_consume_lsx_events(
    runtime: *mut *mut Runtime,
    mx: *mut *const c_void,
    events_out: *mut *mut *const c_char,
    event_pids_out: *mut *mut c_uint,
    event_count_out: *mut c_uint,
) -> usize {
    if runtime.is_null() || mx.is_null() {
        return ERR_INVALID_ARGUMENT;
    }

    let events = unsafe {
        let maxima_arc = Arc::from_raw(*mx as *const Mutex<Maxima>);

        let rt = Box::from_raw(*runtime);
        let result = rt.block_on(async { maxima_arc.lock().await.consume_pending_events() });

        *runtime = Box::into_raw(rt);
        *mx = Arc::into_raw(maxima_arc) as *const c_void;
        result
    };

    let mut c_strings = Vec::with_capacity(events.len());
    let mut c_pids = Vec::with_capacity(events.len());
    for event in events.iter() {
        let (pid, lsx_request) = if let MaximaEvent::ReceivedLSXRequest(pid, request) = event {
            (pid, request)
        } else {
            continue;
        };

        let name: &'static str = lsx_request.into();
        c_strings.push(CString::new(name).unwrap());
        c_pids.push(*pid);
    }

    let mut raw_strings = Vec::with_capacity(c_strings.len());
    for s in c_strings {
        raw_strings.push(s.into_raw());
    }

    unsafe {
        *events_out = Box::into_raw(raw_strings.into_boxed_slice()) as *mut *const c_char;
        *event_pids_out = Box::into_raw(c_pids.into_boxed_slice()) as *mut u32;
        *event_count_out = events.len() as u32;
    }

    ERR_SUCCESS
}

/// Free LSX events retrieved from [maxima_mx_consume_lsx_events].
#[no_mangle]
pub unsafe extern "C" fn maxima_mx_free_lsx_events(events: *mut *mut c_char, event_count: c_uint) {
    let slice = slice::from_raw_parts_mut(events, event_count as usize);
    for &mut raw_str in slice.iter_mut() {
        drop(CString::from_raw(raw_str));
    }

    drop(Box::from_raw(slice));
}

/// Launch a game with Maxima, providing an EA Offer ID.
#[no_mangle]
pub extern "C" fn maxima_launch_game(
    runtime: *mut *mut Runtime,
    mx: *mut *const c_void,
    c_offer_id: *const c_char,
) -> usize {
    if runtime.is_null() || mx.is_null() || c_offer_id.is_null() {
        return ERR_INVALID_ARGUMENT;
    }

    let result = unsafe {
        let maxima_arc = Arc::from_raw(*mx as *const Mutex<Maxima>);

        let rt = Box::from_raw(*runtime);
        let result = rt.block_on(async {
            let offer_id = parse_raw_string(c_offer_id);
            launch::start_game(&offer_id, None, vec![], maxima_arc.clone()).await
        });

        *runtime = Box::into_raw(rt);
        *mx = Arc::into_raw(maxima_arc) as *const c_void;
        result
    };

    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    ERR_SUCCESS
}

/// Find an owned game's offer ID by its slug.
#[no_mangle]
pub extern "C" fn maxima_find_owned_offer(
    runtime: *mut *mut Runtime,
    mx: *mut *const c_void,
    c_game_slug: *const c_char,
    offer_id_out: *mut *const c_char,
) -> usize {
    if runtime.is_null() || mx.is_null() || c_game_slug.is_null() || offer_id_out.is_null() {
        return ERR_INVALID_ARGUMENT;
    }

    let result = unsafe {
        let maxima_arc = Arc::from_raw(*mx as *const Mutex<Maxima>);

        let rt = Box::from_raw(*runtime);
        let result = rt.block_on(async {
            let game_slug = parse_raw_string(c_game_slug);

            let maxima = maxima_arc.lock().await;
            let owned_games = maxima.owned_games(1).await?;
            for game in owned_games.owned_game_products().as_ref().unwrap().items() {
                if game.product().game_slug() != &game_slug {
                    continue;
                }

                if !game.product().downloadable() {
                    continue;
                }

                return Ok(game.origin_offer_id().to_owned());
            }

            bail!("Failed to find game");
        });

        *runtime = Box::into_raw(rt);
        *mx = Arc::into_raw(maxima_arc) as *const c_void;
        result
    };

    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    unsafe {
        *offer_id_out = CString::new(result.unwrap()).unwrap().into_raw();
    }

    ERR_SUCCESS
}

/// Get the local user's display name.
#[no_mangle]
pub extern "C" fn maxima_get_local_display_name(
    runtime: *mut *mut Runtime,
    mx: *mut *const c_void,
    display_name_out: *mut *const c_char,
) -> usize {
    if runtime.is_null() || mx.is_null() || display_name_out.is_null() {
        return ERR_INVALID_ARGUMENT;
    }

    let result = unsafe {
        let maxima_arc = Arc::from_raw(*mx as *const Mutex<Maxima>);

        let rt = Box::from_raw(*runtime);
        let result: Result<String> = rt.block_on(async {
            let maxima = maxima_arc.lock().await;
            let user = maxima.local_user().await?;
            return Ok(user.player().as_ref().unwrap().display_name().to_owned());
        });

        *runtime = Box::into_raw(rt);
        *mx = Arc::into_raw(maxima_arc) as *const c_void;
        result
    };

    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    unsafe {
        *display_name_out = CString::new(result.unwrap()).unwrap().into_raw();
    }

    ERR_SUCCESS
}

/// Pull the application's window into the foreground.
#[no_mangle]
pub extern "C" fn maxima_take_foreground_focus() -> usize {
    let result = take_foreground_focus();
    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    ERR_SUCCESS
}

/// Read the path for an EA game.
#[no_mangle]
pub extern "C" fn maxima_read_game_path(
    c_name: *const c_char,
    c_out_path: *mut *const c_char,
) -> usize {
    if c_name.is_null() {
        return ERR_INVALID_ARGUMENT;
    }

    let name = unsafe { parse_raw_string(c_name) };
    let result = read_game_path(&name);
    if result.is_err() {
        set_last_error_from_result(result);
        return ERR_CHECK_LE;
    }

    unsafe {
        *c_out_path = CString::new(result.unwrap().to_str().unwrap())
            .unwrap()
            .into_raw();
    }

    ERR_SUCCESS
}

/// Send a request to the EA Service Layer
// fn maxima_send_service_request<T, R>(
//     runtime: *mut *mut Runtime,
//     mx: *mut *const c_void,
//     token: *const c_char,
//     operation: &GraphQLRequest,
//     variables: T,
//     response_out: *mut R,
// ) -> usize
// where
//     T: Serialize,
//     R: for<'a> Deserialize<'a>,
// {
//     if runtime.is_null() || mx.is_null() || token.is_null() {
//         return ERR_INVALID_ARGUMENT;
//     }

//     let result = unsafe {
//         let maxima_arc = Arc::from_raw(*mx as *const Mutex<Maxima>);

//         let rt = Box::from_raw(*runtime);
//         let result = rt.block_on(async {
//             let token = parse_raw_string(token);
//             send_service_request::<T, R>(&token, operation, variables).await
//         });

//         *runtime = Box::into_raw(rt);
//         *mx = Arc::into_raw(maxima_arc) as *const c_void;
//         result
//     };

//     if result.is_err() {
//         return ERR_UNKNOWN;
//     }

//     ERR_SUCCESS
// }

// TODO: Need to find a good way to do this
/* #[no_mangle]
pub extern "C" fn maxima_service_layer_get_user_player(
    runtime: *mut *mut Runtime,
    mx: *mut *const c_void,
    token: *const c_char,
    response_out: *mut maxima::core::service_layer::$response,
) -> usize {
    use maxima::core::service_layer::[<SERVICE_REQUEST_ $operation:upper>];
    maxima_send_service_request(runtime, mx, token, [<SERVICE_REQUEST_ $operation:upper>], variables, response_out)
}

define_native_service_request!(GetBasicPlayer, ServiceGetBasicPlayerRequest, ServicePlayer);

macro_rules! define_native_service_request {
    ($operation:expr, $request:ident, $response:ident) => {
        paste::paste! {
            #[no_mangle]
            pub extern "C" fn [<maxima_send_service_request_ $operation:snake:lower>](
                runtime: *mut *mut Runtime,
                mx: *mut *const c_void,
                token: *const c_char,
                variables: maxima::core::service_layer::$request,
                response_out: *mut maxima::core::service_layer::$response,
            ) -> usize {
                use maxima::core::service_layer::[<SERVICE_REQUEST_ $operation:upper>];
                maxima_send_service_request(runtime, mx, token, [<SERVICE_REQUEST_ $operation:upper>], variables, response_out)
            }
        }
    };
} */

unsafe fn parse_raw_string(buf: *const c_char) -> String {
    let c_str = CStr::from_ptr(buf);
    let str_slice = c_str.to_str().unwrap();
    str_slice.to_owned()
}
