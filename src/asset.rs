use libc::{off_t, off64_t};
use ndk_sys::{AAsset, AAssetManager};

use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::{CStr, CString, OsStr},
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    ops::{Deref, DerefMut},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
    sync::{Arc, LazyLock, Mutex, OnceLock},
};

// This makes me feel wrong... but all we will do is compare the pointer
// and the struct will be used in a mutex so i guess this is safe??
#[derive(PartialEq, Eq, Hash)]
struct AAssetPtr(*const ndk_sys::AAsset);
unsafe impl Send for AAssetPtr {}
trait CustomFile {
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
        self.seek(SeekFrom::Current(0))
    }
    fn rem(&mut self) -> io::Result<u64> {
        Ok(self.len()? - self.stream_position()?)
    }
}
// The assets we have registrered to remplace data about
static WANTED_ASSETS: LazyLock<Mutex<HashMap<AAssetPtr, Box<dyn CustomFile + Sync + Send>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) unsafe fn open(
    man: *mut AAssetManager,
    fname: *const libc::c_char,
    mode: libc::c_int,
) -> *mut ndk_sys::AAsset {
    // This is where ub can happen, but we are merely a hook.
    let aasset = unsafe { ndk_sys::AAssetManager_open(man, fname, mode) };
    let c_str = unsafe { CStr::from_ptr(fname) };
    let raw_cstr = c_str.to_bytes();
    let os_str = OsStr::from_bytes(raw_cstr);
    let c_path: &Path = Path::new(os_str);
    return aasset;
}
/// Join paths without allocating if possible, or
/// if the joined path does not fit the buffer then just
/// allocate instead
fn opt_path_join<'a>(bytes: &'a mut [u8; 128], paths: &[&Path]) -> Cow<'a, CStr> {
    let total_len: usize = paths.iter().map(|p| p.as_os_str().len()).sum();
    if total_len + 1 > 128 {
        // panic!("fuck");
        let mut pathbuf = PathBuf::new();
        for path in paths {
            pathbuf.push(path);
        }
        let cpath = CString::new(pathbuf.into_os_string().as_encoded_bytes()).unwrap();
        return Cow::Owned(cpath);
    }

    let mut writer = bytes.as_mut_slice();
    for path in paths {
        let osstr = path.as_os_str().as_bytes();
        writer.write(osstr).unwrap();
    }
    writer.write(&[0]).unwrap();
    let guh = CStr::from_bytes_until_nul(bytes).unwrap();
    Cow::Borrowed(guh)
}

pub(crate) unsafe fn seek64(aasset: *mut AAsset, off: off64_t, whence: libc::c_int) -> off64_t {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_seek64(aasset, off, whence),
    };
    seek_facade(off, whence, file) as off64_t
}

pub(crate) unsafe fn seek(aasset: *mut AAsset, off: off_t, whence: libc::c_int) -> off_t {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_seek(aasset, off, whence),
    };
    // This code can be very deadly on large files,
    // but since NO replacement should surpass u32 max we should be fine...
    // i dont even think a mcpack can exceed that
    seek_facade(off.into(), whence, file) as off_t
}

pub(crate) unsafe fn read(
    aasset: *mut AAsset,
    buf: *mut libc::c_void,
    count: libc::size_t,
) -> libc::c_int {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_read(aasset, buf, count),
    };
    // Reuse buffer given by caller
    let rs_buffer = core::slice::from_raw_parts_mut(buf as *mut u8, count);
    let read_total = match file.read(rs_buffer) {
        Ok(n) => n,
        Err(e) => {
            //            log::warn!("failed fake aaset read: {e}");
            return -1 as libc::c_int;
        }
    };
    read_total as libc::c_int
}

pub(crate) unsafe fn len(aasset: *mut AAsset) -> off_t {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getLength(aasset),
    };
    file.len().unwrap() as off_t
}

pub(crate) unsafe fn len64(aasset: *mut AAsset) -> off64_t {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let mut file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getLength64(aasset),
    };
    file.len().unwrap() as off64_t
}

pub(crate) unsafe fn rem(aasset: *mut AAsset) -> off_t {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let mut file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getRemainingLength(aasset),
    };
    file.rem().unwrap() as off_t
}

pub(crate) unsafe fn rem64(aasset: *mut AAsset) -> off64_t {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getRemainingLength64(aasset),
    };
    file.rem().unwrap() as off64_t
}

pub(crate) unsafe fn close(aasset: *mut AAsset) {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    if wanted_assets.remove(&AAssetPtr(aasset)).is_none() {
        ndk_sys::AAsset_close(aasset);
    }
}

pub(crate) unsafe fn get_buffer(aasset: *mut AAsset) -> *const libc::c_void {
    let mut wanted_assets = WANTED_ASSETS.lock().unwrap();
    let file = match wanted_assets.get_mut(&AAssetPtr(aasset)) {
        Some(file) => file,
        None => return ndk_sys::AAsset_getBuffer(aasset),
    };
    // Lets hope this does not go boom boom
    ptr::null()
}

pub(crate) unsafe fn fd_dummy(
    aasset: *mut AAsset,
    out_start: *mut off_t,
    out_len: *mut off_t,
) -> libc::c_int {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(_) => {
            //            log::error!("WE GOT BUSTED NOOO");
            -1
        }
        None => ndk_sys::AAsset_openFileDescriptor(aasset, out_start, out_len),
    }
}

pub(crate) unsafe fn fd_dummy64(
    aasset: *mut AAsset,
    out_start: *mut off64_t,
    out_len: *mut off64_t,
) -> libc::c_int {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(_) => {
            //            log::error!("WE GOT BUSTED NOOO");
            -1
        }
        None => ndk_sys::AAsset_openFileDescriptor64(aasset, out_start, out_len),
    }
}

pub(crate) unsafe fn is_alloc(aasset: *mut AAsset) -> libc::c_int {
    let wanted_assets = WANTED_ASSETS.lock().unwrap();
    match wanted_assets.get(&AAssetPtr(aasset)) {
        Some(_) => false as libc::c_int,
        None => ndk_sys::AAsset_isAllocated(aasset),
    }
}

fn seek_facade(
    offset: i64,
    whence: libc::c_int,
    fil: &mut Box<dyn CustomFile + Send + Sync>,
) -> i64 {
    let offset = match whence {
        libc::SEEK_SET => {
            //Lets check this so we dont mess up
            let u64_off = match u64::try_from(offset) {
                Ok(uoff) => uoff,
                Err(e) => {
                    //                    log::error!("signed ({offset}) to unsigned failed: {e}");
                    return -1;
                }
            };
            io::SeekFrom::Start(u64_off)
        }
        libc::SEEK_CUR => io::SeekFrom::Current(offset),
        libc::SEEK_END => io::SeekFrom::End(offset),
        _ => {
            //            log::error!("Invalid seek whence");
            return -1;
        }
    };
    match fil.seek(offset) {
        Ok(new_offset) => match new_offset.try_into() {
            Ok(int) => int,
            Err(err) => {
                //                log::error!("u64 ({new_offset}) to i64 failed: {err}");
                -1
            }
        },
        Err(err) => {
            //            log::error!("aasset seek failed: {err}");
            -1
        }
    }
}
