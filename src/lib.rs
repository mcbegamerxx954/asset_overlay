use crate::asset::FILE_PROVIDERS;
//use crate::asset::SyncProvider;
//use crate::plthook::replace_plt_functions;
use modutils::Module;
//use plt_rs::DynamicLibrary;
mod asset;
//mod plthook;
pub use asset::CustomFile;
pub use asset::FileProvider;
pub use asset::SyncFile;
pub use asset::SyncProvider;
//use smallbox::SmallBox;
// use smallbox::space::S8;
// pub use smallbox::*;
macro_rules! cast_array {
    ($($func_name:literal -> $hook:expr),
        *,
    ) => {
        [
            $(($func_name, $hook as *const u8)),*,
        ]
    }
}
pub fn register_provider(thing: Box<SyncProvider>) {
    let mut sus = FILE_PROVIDERS.lock().unwrap();
    sus.push(thing);
}
pub fn hook_aaset(lib: &mut Module) {
    let asset_fn_list = cast_array! {
        "AAssetManager_open" -> asset::open,
        "AAsset_read" -> asset::read,
        "AAsset_close" -> asset::close,
        "AAsset_seek" -> asset::seek,
        "AAsset_seek64" -> asset::seek64,
        "AAsset_getLength" -> asset::len,
        "AAsset_getLength64" -> asset::len64,
        "AAsset_getRemainingLength" -> asset::rem,
        "AAsset_getRemainingLength64" -> asset::rem64,
        "AAsset_openFileDescriptor" -> asset::fd_dummy,
        "AAsset_openFileDescriptor64" -> asset::fd_dummy64,
        "AAsset_getBuffer" -> asset::get_buffer,
        "AAsset_isAllocated" -> asset::is_alloc,
    };
    for (name, hook) in asset_fn_list {
        lib.replace_lib_import(name, hook);
    }
}
