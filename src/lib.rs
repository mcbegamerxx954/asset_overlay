use crate::asset::FILE_PROVIDERS;
//use crate::asset::SyncProvider;
use crate::plthook::replace_plt_functions;
use plt_rs::DynamicLibrary;
mod asset;
mod plthook;
pub use asset::CustomFile;
pub use asset::FileProvider;
pub use asset::SyncFile;
pub use asset::SyncProvider;
//use smallbox::SmallBox;
use smallbox::space::S8;
pub use smallbox::*;
macro_rules! cast_array {
    ($($func_name:literal -> $hook:expr),
        *,
    ) => {
        [
            $(($func_name, $hook as *const u8)),*,
        ]
    }
}
pub fn register_provider(thing: SmallBox<SyncProvider, S8>) {
    let mut sus = FILE_PROVIDERS.lock().unwrap();
    sus.push(thing);
}
pub fn hook_aaset(libname: &str) {
    let lib_entry = find_lib(libname).expect("Cannot find minecraftpe");
    let dyn_lib = DynamicLibrary::initialize(lib_entry).expect("Failed to find mc info");
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
    replace_plt_functions(&dyn_lib, asset_fn_list);
}

fn find_lib<'a>(target_name: &str) -> Option<plt_rs::LoadedLibrary<'a>> {
    let loaded_modules = plt_rs::collect_modules();
    loaded_modules
        .into_iter()
        .find(|lib| lib.name().contains(target_name))
}
