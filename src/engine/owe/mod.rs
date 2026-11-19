//! OWE hosts one client-owned task on the window thread.
mod host;
mod world;
use std::{cell::RefCell, rc::Rc};
use world::{Packet, World, INVALID};

type Resume = Box<dyn FnMut(u32, u32, u32, u32, bool) -> i32>;
thread_local! {
    static ACTIVE: RefCell<Option<Rc<RefCell<World>>>> = const { RefCell::new(None) };
}

fn call(p: &mut Packet) -> i32 {
    ACTIVE.with_borrow(|w| {
        w.as_ref()
            .map_or(INVALID, |world| world.borrow_mut().call(p))
    })
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
unsafe extern "C" fn owe_call_v1(packet: *mut Packet) -> i32 {
    packet.as_mut().map_or(INVALID, call)
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
unsafe extern "C" fn owe_run_v1(
    path: *const std::ffi::c_char,
    resume: Option<extern "C" fn(u32, u32, u32, u32, u32) -> i32>,
) -> i32 {
    let run = || -> Result<(), String> {
        let resume = resume.ok_or("missing client entry point")?;
        if path.is_null() {
            return Err("missing game package path".into());
        }
        let path = std::ffi::CStr::from_ptr(path)
            .to_str()
            .map_err(|e| e.to_string())?;
        let json = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        host::run(
            World::new(&json)?,
            Box::new(move |a, b, c, d, e| resume(a, b, c, d, u32::from(e))),
        )
    };
    match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("OWE: {e}");
            1
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    pub fn owe_call_v1(bytes: &mut [u8]) -> i32 {
        if bytes.len() != std::mem::size_of::<Packet>() {
            return INVALID;
        }
        let mut p = bytemuck::pod_read_unaligned(bytes);
        let status = call(&mut p);
        bytes.copy_from_slice(bytemuck::bytes_of(&p));
        status
    }
    #[wasm_bindgen]
    pub fn owe_start_v1(json: &str, resume: js_sys::Function) -> Result<(), JsValue> {
        console_error_panic_hook::set_once();
        let client: Resume = Box::new(move |a, b, c, d, e| {
            let args = js_sys::Array::of5(
                &a.into(),
                &b.into(),
                &c.into(),
                &d.into(),
                &u32::from(e).into(),
            );
            match resume.apply(&JsValue::UNDEFINED, &args) {
                Ok(v) => v.as_f64().unwrap_or(-1.) as i32,
                Err(err) => {
                    web_sys::console::error_1(&err);
                    -1
                }
            }
        });
        host::run(World::new(json)?, client).map_err(Into::into)
    }
}
