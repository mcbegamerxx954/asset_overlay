//Explanation: Aasset is NOT thread-safe anyways so we will not try adding thread safety either
#![allow(static_mut_refs)]
#![allow(non_snake_case)]
use libc::{off_t, off64_t};
use ndk::asset::AssetManager;
use ndk_sys::{AAsset, AAssetManager};
use std::{
    cell::UnsafeCell,
    ffi::{CStr, OsStr},
    io::{self, Read, Seek, SeekFrom},
    os::unix::ffi::OsStrExt,
    path::Path,
    ptr::{self, NonNull},
    sync::{LazyLock, Mutex},
};

pub type SyncFile = dyn CustomFile + Sync + Send;
pub type SyncProvider = dyn FileProvider + Sync + Send;

pub trait FileProvider {
    fn get_file(&mut self, name: &Path, man: &AssetManager) -> Option<Box<SyncFile>>;
}

pub static FILE_PROVIDERS: LazyLock<Mutex<Vec<Box<SyncProvider>>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

pub trait CustomFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    fn seek(&mut self, seek: SeekFrom) -> io::Result<u64>;
    fn len(&mut self) -> io::Result<u64>;
    fn rem(&mut self) -> io::Result<u64>;
}

impl<T: Read + Seek> CustomFile for T {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read(buf)
    }
    fn seek(&mut self, seek: SeekFrom) -> io::Result<u64> {
        self.seek(seek)
    }
    fn len(&mut self) -> io::Result<u64> {
        let curr = self.seek(SeekFrom::Current(0))?;
        let len = self.seek(SeekFrom::End(0))?;
        self.seek(SeekFrom::Start(curr))?;
        Ok(len)
    }
    fn rem(&mut self) -> io::Result<u64> {
        let current_pos = self.seek(SeekFrom::Current(0))?;
        let len = self.seek(SeekFrom::End(0))?;
        let rem = len - current_pos;
        self.seek(SeekFrom::Start(current_pos))?;
        Ok(rem)
    }
}

// The assets we have registrered to remplace data about
static mut WANTED_ASSETS: UnsafeCell<Vec<*mut Box<SyncFile>>> = UnsafeCell::new(Vec::new());

pub(crate) unsafe fn open(
    man: *mut AAssetManager,
    fname: *const libc::c_char,
    mode: libc::c_int,
) -> *mut ndk_sys::AAsset {
    // This is where ub can happen, but we are merely a hook.

    let c_str = unsafe { CStr::from_ptr(fname) };
    let raw_cstr = c_str.to_bytes();
    let os_str = OsStr::from_bytes(raw_cstr);
    let c_path: &Path = Path::new(os_str);

    let sus = unsafe { AssetManager::from_ptr(NonNull::new(man).unwrap()) };
    let mut providers = FILE_PROVIDERS.lock().unwrap();
    for provider in providers.iter_mut() {
        if let Some(file) = provider.get_file(c_path, &sus) {
            let wanted = unsafe { WANTED_ASSETS.get_mut() };
            let extrabox = Box::new(file);
            let pointer = Box::into_raw(extrabox);
            wanted.push(pointer);
            return pointer.cast();
            //            wanted.insert(AAssetPtr(aasset), file);
        }
    }
    unsafe { ndk_sys::AAssetManager_open(man, fname, mode) }
}

macro_rules! aah {
    ((ptr: $ptr:ident, matched_file: $file:ident) $(pub unsafe fn $name:ident($($arg_name:ident : $arg_ty:ty),*) -> $ret_type:ty = $body:expr),*) => {
        $(pub unsafe fn $name($($arg_name:$arg_ty),*) -> $ret_type {
            let  wanted_assets = unsafe { WANTED_ASSETS.get_mut() };
            let $file = if wanted_assets.contains(&$ptr.cast()) {
                let pointer = $ptr as *mut Box<SyncFile>;
                unsafe {pointer.as_mut().unwrap()}
            } else {
                return unsafe { ndk_sys::$name($($arg_name),*) };
            };
            $body
        })*

    };
}

aah! {(ptr: aasset, matched_file: file)
pub unsafe fn AAsset_seek64(aasset: *mut AAsset, off: off64_t, whence: libc::c_int) -> off64_t ={
    seek_facade(off.into(), whence, file)
},

pub unsafe fn AAsset_seek(aasset: *mut AAsset, off: off_t, whence: libc::c_int) -> off_t ={
    // This code can be very deadly on large files,
    // but since NO replacement should surpass u32 max we should be fine...
    // i dont even think a mcpack can exceed that
    seek_facade(off.into(), whence, file) as off_t
},

pub unsafe fn AAsset_read(
    aasset: *mut AAsset,
    buf: *mut libc::c_void,
    count: libc::size_t) -> libc::c_int = {
    unsafe {
        // Reuse buffer given by caller
        let rs_buffer = core::slice::from_raw_parts_mut(buf as *mut u8, count);
        match file.read(rs_buffer) {
            Ok(n) => n as libc::c_int,
            Err(_e) => -1 as libc::c_int
        }
    }
},

pub unsafe fn AAsset_getLength(aasset: *mut AAsset) -> off_t ={
    file.len().unwrap() as off_t
},

pub unsafe fn AAsset_getLength64(aasset: *mut AAsset) -> off64_t ={
    file.len().unwrap() as off64_t
},

pub unsafe fn AAsset_getRemainingLength(aasset: *mut AAsset) -> off_t = {
    file.rem().unwrap() as off_t
},

pub unsafe fn AAsset_getRemainingLength64(aasset: *mut AAsset) -> off64_t ={
    file.rem().unwrap() as off64_t
},

pub unsafe fn AAsset_getBuffer(aasset: *mut AAsset) -> *const libc::c_void  = {
    // TODO: We have no good way of implementing this...
    ptr::null()
},

pub unsafe fn AAsset_openFileDescriptor(
    aasset: *mut AAsset,
    out_start: *mut off_t,
    out_len: *mut off_t
) -> libc::c_int  = {
    // TODO: We are cooked
    return -1;
},


pub unsafe fn AAsset_openFileDescriptor64(
    aasset: *mut AAsset,
    out_start: *mut off64_t,
    out_len: *mut off64_t
) -> libc::c_int = {
    // TODO: We are cooked
    return -1;

},

pub unsafe fn AAsset_isAllocated(aasset: *mut AAsset) -> libc::c_int = {
    // TODO: How do we even do this..
    return false as libc::c_int;
}
}

pub(crate) unsafe fn close(aasset: *mut AAsset) {
    unsafe {
        let wanted_assets = WANTED_ASSETS.get_mut();
        match wanted_assets.iter().position(|p| *p == aasset.cast()) {
            Some(yay) => {
                wanted_assets.remove(yay);
                let boxed = aasset.cast::<Box<SyncFile>>();
                // Idk if this works but it should be freeing the data once this function finishes
                let _boxi = Box::from_raw(boxed);
                // Just in case
                drop(_boxi);
            }
            None => return ndk_sys::AAsset_close(aasset),
        };
    }
}
fn seek_facade(offset: i64, whence: libc::c_int, fil: &mut Box<SyncFile>) -> i64 {
    let offset = match whence {
        libc::SEEK_SET => {
            //Lets check this so we dont mess up
            let u64_off = match u64::try_from(offset) {
                Ok(uoff) => uoff,
                Err(_e) => return -1,
            };
            io::SeekFrom::Start(u64_off)
        }
        libc::SEEK_CUR => io::SeekFrom::Current(offset),
        libc::SEEK_END => io::SeekFrom::End(offset),
        _ => return -1,
    };
    match fil.seek(offset) {
        Ok(new_offset) => match new_offset.try_into() {
            Ok(int) => int,
            Err(_err) => -1,
        },
        Err(_err) => -1,
    }
}
