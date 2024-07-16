use std::{
    os::raw::{c_char, c_int, c_uchar, c_uint, c_ulong, c_ushort, c_void},
    ptr,
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use flate2::raw::{gz_headerp, mz_stream, z_streamp};
use log::error;

pub const Z_ENOUGH_LENS: usize = 852;
pub const Z_ENOUGH_DISTS: usize = 592;
pub const Z_ENOUGH: usize = Z_ENOUGH_LENS + Z_ENOUGH_DISTS;

const Z_MAGIC: u32 = u32::from_be_bytes(*b"ZSTA");

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ZCode {
    pub op: c_uchar,
    pub bits: c_uchar,
    pub val: c_ushort,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ZInflateState {
    pub strm: z_streamp,
    pub inflate_mode: c_uint,

    pub last: c_int,
    pub wrap: c_int,

    pub havedict: c_int,
    pub flags: c_int,

    pub dmax: c_uint,
    pub check: c_ulong,
    pub total: c_ulong,
    pub head: gz_headerp,

    pub wbits: c_uint,
    pub wsize: c_uint,
    pub whave: c_uint,
    pub wnext: c_uint,
    pub window: *mut c_char,

    pub hold: c_ulong,
    pub bits: c_uint,

    pub length: c_uint,
    pub offset: c_uint,
    pub extra: c_uint,

    pub lencode: *mut ZCode,
    pub distcode: *mut ZCode,

    pub lenbits: c_uint,
    pub distbits: c_uint,

    pub ncode: c_uint,
    pub nlen: c_uint,
    pub ndist: c_uint,
    pub have: c_uint,
    pub next: *mut ZCode,

    pub lens: [c_ushort; 320],
    pub work: [c_ushort; 288],

    pub codes: [ZCode; Z_ENOUGH],

    pub sane: c_int,
    pub back: c_int,
    pub was: c_uint,
}

pub(crate) fn write_zlib_state(buf: &mut BytesMut, stream: &mut mz_stream) {
    buf.put_u32(Z_MAGIC);

    buf.put_u64(stream.total_in as u64);
    buf.put_u64(stream.total_out as u64);
    buf.put_i32(stream.data_type as i32);
    buf.put_u64(stream.adler as u64);

    let state = stream.state as *mut ZInflateState;
    let state_ref = unsafe { &mut *state };

    let size = std::mem::size_of::<ZInflateState>();
    let mut buffer = vec![0; size];
    unsafe {
        ptr::copy_nonoverlapping(state, buffer.as_mut_ptr() as *mut ZInflateState, 1);
    }

    for byte in buffer {
        buf.put_u8(byte);
    }

    if !state_ref.window.is_null() {
        let window_size = 1 << state_ref.wbits;
        let mut window_buffer = vec![0; window_size];
        unsafe {
            ptr::copy_nonoverlapping(state_ref.window, window_buffer.as_mut_ptr(), window_size);
        }

        for byte in window_buffer {
            buf.put_i8(byte);
        }
    }

    let mut lencode_index = unsafe { state_ref.lencode.offset_from(state_ref.codes.as_ptr()) };
    let mut distcode_index = unsafe { state_ref.distcode.offset_from(state_ref.codes.as_ptr()) };
    let mut next_index = unsafe { state_ref.next.offset_from(state_ref.codes.as_ptr()) };

    if lencode_index > Z_ENOUGH.try_into().unwrap() {
        lencode_index = 0;
        //panic!("Can't serialize this zlib state, lencode too high! (Lencode: [{} {:?} {:?}], Distcode: {}, Nextcode: {})", lencode_index, state_ref.lencode, state_ref.codes.as_ptr(), distcode_index, next_index);
    }

    if distcode_index > Z_ENOUGH.try_into().unwrap() {
        distcode_index = 0;
        //panic!("Can't serialize this zlib state, lencode too high! (Lencode: [{} {:?} {:?}], Distcode: {}, Nextcode: {})", lencode_index, state_ref.lencode, state_ref.codes.as_ptr(), distcode_index, next_index);
    }

    if next_index > Z_ENOUGH.try_into().unwrap() {
        next_index = 0;
        //panic!("Can't serialize this zlib state, lencode too high! (Lencode: [{} {:?} {:?}], Distcode: {}, Nextcode: {})", lencode_index, state_ref.lencode, state_ref.codes.as_ptr(), distcode_index, next_index);
    }

    buf.put_u32(lencode_index as u32);
    buf.put_u32(distcode_index as u32);
    buf.put_u32(next_index as u32);

    buf.put_u32(state_ref.lenbits);
    buf.put_u32(state_ref.distbits);
}

pub(crate) fn restore_zlib_state(buf: &mut Bytes, stream: &mut mz_stream) {
    if buf.get_u32() != Z_MAGIC {
        error!("Invalid magic number while reading zlib state");
        return;
    }

    stream.total_in = buf.get_u64();
    stream.total_out = buf.get_u64();
    stream.data_type = buf.get_i32();
    stream.adler = buf.get_u64();

    let state = stream.state as *mut ZInflateState;
    let state_ref = unsafe { &mut *state };

    let size = std::mem::size_of::<ZInflateState>();
    for i in 0..size {
        let byte = buf.get_u8();
        unsafe {
            ptr::copy_nonoverlapping(
                &byte,
                (state as *mut c_uchar).offset(i.try_into().unwrap()),
                1,
            );
        }
    }
    unsafe {
        (*state).strm = &mut *stream;
    }

    if !state_ref.window.is_null() {
        let window_size = 1 << state_ref.wbits;

        let streamp = stream as z_streamp;
        state_ref.window =
            unsafe { (stream.zalloc)(streamp as *mut c_void, 1, window_size) } as *mut i8;

        for i in 0..window_size {
            let byte = buf.get_u8();
            unsafe {
                ptr::copy_nonoverlapping(
                    &byte,
                    state_ref.window.offset(i.try_into().unwrap()) as *mut u8,
                    1,
                );
            }
        }
    }

    let lencode = buf.get_u32() as isize;
    let distcode = buf.get_u32() as isize;
    let nextcode = buf.get_u32() as isize;

    if lencode > Z_ENOUGH.try_into().unwrap() {
        panic!("Can't deserialize this zlib state, lencode too high!");
    }

    state_ref.lencode = unsafe { state_ref.codes.as_ptr().offset(lencode) as *mut ZCode };
    state_ref.distcode = unsafe { state_ref.codes.as_ptr().offset(distcode) as *mut ZCode };
    state_ref.next = unsafe { state_ref.codes.as_ptr().offset(nextcode) as *mut ZCode };

    state_ref.lenbits = buf.get_u32();
    state_ref.distbits = buf.get_u32();
}