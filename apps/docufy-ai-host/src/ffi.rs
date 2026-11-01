//! The ONNX Runtime GenAI C API (`ort_genai_c.h`, version 0.14), loaded at run time from the
//! folder Docufy downloaded it to, behind a safe interface: [`Genai`] loads the library,
//! [`Model`], [`Tokenizer`] and [`Generator`] own their handles and free them on drop.
//!
//! This is the only code in Docufy with `unsafe`. Every call passes handles this module created
//! and still owns, and C strings it keeps alive for the duration of the call.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;
use std::sync::Arc;

type Res = *mut c_void;
type Handle = *mut c_void;

/// The functions Docufy uses.
struct Api {
    result_get_error: unsafe extern "C" fn(*const c_void) -> *const c_char,
    destroy_result: unsafe extern "C" fn(Res),
    destroy_string: unsafe extern "C" fn(*const c_char),
    create_config: unsafe extern "C" fn(*const c_char, *mut Handle) -> Res,
    destroy_config: unsafe extern "C" fn(Handle),
    config_clear_providers: unsafe extern "C" fn(Handle) -> Res,
    config_append_provider: unsafe extern "C" fn(Handle, *const c_char) -> Res,
    config_set_provider_option: unsafe extern "C" fn(Handle, *const c_char, *const c_char, *const c_char) -> Res,
    create_model_from_config: unsafe extern "C" fn(Handle, *mut Handle) -> Res,
    destroy_model: unsafe extern "C" fn(Handle),
    model_get_device_type: unsafe extern "C" fn(Handle, *mut *const c_char) -> Res,
    create_tokenizer: unsafe extern "C" fn(Handle, *mut Handle) -> Res,
    destroy_tokenizer: unsafe extern "C" fn(Handle),
    create_sequences: unsafe extern "C" fn(*mut Handle) -> Res,
    destroy_sequences: unsafe extern "C" fn(Handle),
    sequences_get_sequence_count: unsafe extern "C" fn(Handle, usize) -> usize,
    tokenizer_encode: unsafe extern "C" fn(Handle, *const c_char, Handle) -> Res,
    create_tokenizer_stream: unsafe extern "C" fn(Handle, *mut Handle) -> Res,
    destroy_tokenizer_stream: unsafe extern "C" fn(Handle),
    tokenizer_stream_decode: unsafe extern "C" fn(Handle, i32, *mut *const c_char) -> Res,
    create_generator_params: unsafe extern "C" fn(Handle, *mut Handle) -> Res,
    destroy_generator_params: unsafe extern "C" fn(Handle),
    generator_params_set_search_number: unsafe extern "C" fn(Handle, *const c_char, f64) -> Res,
    create_generator: unsafe extern "C" fn(Handle, Handle, *mut Handle) -> Res,
    destroy_generator: unsafe extern "C" fn(Handle),
    generator_is_done: unsafe extern "C" fn(Handle) -> bool,
    generator_append_token_sequences: unsafe extern "C" fn(Handle, Handle) -> Res,
    generator_generate_next_token: unsafe extern "C" fn(Handle) -> Res,
    generator_get_next_tokens: unsafe extern "C" fn(Handle, *mut *const i32, *mut usize) -> Res,
    shutdown: unsafe extern "C" fn(),
    // Last: the functions above point into it.
    _lib: libloading::Library,
}

/// The loaded library.
#[derive(Clone)]
pub struct Genai(Arc<Api>);

/// The library's file name on this system.
pub const LIBRARY: &str = if cfg!(windows) {
    "onnxruntime-genai.dll"
} else if cfg!(target_os = "macos") {
    "libonnxruntime-genai.dylib"
} else {
    "libonnxruntime-genai.so"
};

fn cstring(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| "text contains a NUL character".to_string())
}

impl Genai {
    /// Loads the library from `dir`, where its dependencies (`onnxruntime`, `DirectML`) lie too.
    pub fn load(dir: &Path) -> Result<Genai, String> {
        let path = dir.join(LIBRARY);
        // SAFETY: loading runs the library's initialisers. The file is Microsoft's ONNX Runtime
        // GenAI, which Docufy downloaded from its official release and checked against a pinned
        // SHA-256 before installing it in `dir`.
        #[cfg(windows)]
        let lib: libloading::Library = unsafe {
            use libloading::os::windows::{LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, Library};
            // Its dependencies are looked for beside it, not on the PATH.
            Library::load_with_flags(&path, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS).map(Into::into)
        }
        .map_err(|e| format!("{}: {e}", path.display()))?;
        // SAFETY: as above.
        #[cfg(not(windows))]
        let lib = unsafe { libloading::Library::new(&path) }.map_err(|e| format!("{}: {e}", path.display()))?;
        macro_rules! sym {
            ($name:literal) => {
                // SAFETY: the type matches the declaration in `ort_genai_c.h` 0.14.
                *unsafe { lib.get(concat!($name, "\0").as_bytes()) }.map_err(|e| format!("{}: {e}", $name))?
            };
        }
        let api = Api {
            result_get_error: sym!("OgaResultGetError"),
            destroy_result: sym!("OgaDestroyResult"),
            destroy_string: sym!("OgaDestroyString"),
            create_config: sym!("OgaCreateConfig"),
            destroy_config: sym!("OgaDestroyConfig"),
            config_clear_providers: sym!("OgaConfigClearProviders"),
            config_append_provider: sym!("OgaConfigAppendProvider"),
            config_set_provider_option: sym!("OgaConfigSetProviderOption"),
            create_model_from_config: sym!("OgaCreateModelFromConfig"),
            destroy_model: sym!("OgaDestroyModel"),
            model_get_device_type: sym!("OgaModelGetDeviceType"),
            create_tokenizer: sym!("OgaCreateTokenizer"),
            destroy_tokenizer: sym!("OgaDestroyTokenizer"),
            create_sequences: sym!("OgaCreateSequences"),
            destroy_sequences: sym!("OgaDestroySequences"),
            sequences_get_sequence_count: sym!("OgaSequencesGetSequenceCount"),
            tokenizer_encode: sym!("OgaTokenizerEncode"),
            create_tokenizer_stream: sym!("OgaCreateTokenizerStream"),
            destroy_tokenizer_stream: sym!("OgaDestroyTokenizerStream"),
            tokenizer_stream_decode: sym!("OgaTokenizerStreamDecode"),
            create_generator_params: sym!("OgaCreateGeneratorParams"),
            destroy_generator_params: sym!("OgaDestroyGeneratorParams"),
            generator_params_set_search_number: sym!("OgaGeneratorParamsSetSearchNumber"),
            create_generator: sym!("OgaCreateGenerator"),
            destroy_generator: sym!("OgaDestroyGenerator"),
            generator_is_done: sym!("OgaGenerator_IsDone"),
            generator_append_token_sequences: sym!("OgaGenerator_AppendTokenSequences"),
            generator_generate_next_token: sym!("OgaGenerator_GenerateNextToken"),
            generator_get_next_tokens: sym!("OgaGenerator_GetNextTokens"),
            shutdown: sym!("OgaShutdown"),
            _lib: lib,
        };
        Ok(Genai(Arc::new(api)))
    }

    /// `Err` with the library's message when `res` is an error; frees it.
    fn check(&self, res: Res) -> Result<(), String> {
        if res.is_null() {
            return Ok(());
        }
        // SAFETY: `res` is a non-null OgaResult the library just returned; its message lives
        // until it is destroyed, after the copy.
        let message = unsafe {
            let text = (self.0.result_get_error)(res);
            let message = if text.is_null() { "unknown error".to_string() } else { CStr::from_ptr(text).to_string_lossy().into_owned() };
            (self.0.destroy_result)(res);
            message
        };
        Err(message)
    }

    /// Ends the library's global state; call once, when nothing of it is in use anymore.
    pub fn shutdown(&self) {
        // SAFETY: called after every handle is dropped (the host's last step).
        unsafe { (self.0.shutdown)() }
    }

    /// Loads the model in `dir` for `provider` (`dml`, `cpu`), with provider `options`.
    pub fn model(&self, dir: &Path, provider: &str, options: &[(&str, &str)]) -> Result<Model, String> {
        let a = &self.0;
        let dir = cstring(&dir.to_string_lossy())?;
        let provider = cstring(provider)?;
        let mut config: Handle = std::ptr::null_mut();
        // SAFETY: valid C strings; `config` receives a new handle, destroyed below.
        self.check(unsafe { (a.create_config)(dir.as_ptr(), &mut config) })?;
        let configured = (|| {
            // SAFETY: `config` is the live handle created above.
            self.check(unsafe { (a.config_clear_providers)(config) })?;
            if provider.as_bytes() != b"cpu" {
                // SAFETY: as above.
                self.check(unsafe { (a.config_append_provider)(config, provider.as_ptr()) })?;
                for (key, value) in options {
                    let (key, value) = (cstring(key)?, cstring(value)?);
                    // SAFETY: as above, with valid C strings.
                    self.check(unsafe { (a.config_set_provider_option)(config, provider.as_ptr(), key.as_ptr(), value.as_ptr()) })?;
                }
            }
            let mut model: Handle = std::ptr::null_mut();
            // SAFETY: as above; `model` receives a new handle owned by the returned `Model`.
            self.check(unsafe { (a.create_model_from_config)(config, &mut model) })?;
            Ok(Model { genai: self.clone(), handle: model })
        })();
        // SAFETY: `config` was created above and is not used after this.
        unsafe { (a.destroy_config)(config) };
        configured
    }
}

pub struct Model {
    genai: Genai,
    handle: Handle,
}

impl Drop for Model {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by this value and dropped once.
        unsafe { (self.genai.0.destroy_model)(self.handle) }
    }
}

impl Model {
    /// The device the model runs on, as the library names it ("DML", "CPU").
    pub fn device_type(&self) -> Result<String, String> {
        let mut out: *const c_char = std::ptr::null();
        // SAFETY: live model handle; the string is copied and then freed with OgaDestroyString.
        self.genai.check(unsafe { (self.genai.0.model_get_device_type)(self.handle, &mut out) })?;
        if out.is_null() {
            return Ok(String::new());
        }
        // SAFETY: `out` is a NUL-terminated string the library allocated.
        let text = unsafe { CStr::from_ptr(out).to_string_lossy().into_owned() };
        // SAFETY: as above; freed once.
        unsafe { (self.genai.0.destroy_string)(out) };
        Ok(text)
    }

    pub fn tokenizer(&self) -> Result<Tokenizer, String> {
        let mut handle: Handle = std::ptr::null_mut();
        // SAFETY: live model handle; the new handle is owned by the returned `Tokenizer`.
        self.genai.check(unsafe { (self.genai.0.create_tokenizer)(self.handle, &mut handle) })?;
        Ok(Tokenizer { genai: self.genai.clone(), handle })
    }

    /// A generator for `prompt` (its tokens), stopping at `max_length` tokens in all.
    pub fn generator(&self, prompt: &Tokens, max_length: usize) -> Result<Generator, String> {
        let a = &self.genai.0;
        let mut params: Handle = std::ptr::null_mut();
        // SAFETY: live model handle; `params` is destroyed below.
        self.genai.check(unsafe { (a.create_generator_params)(self.handle, &mut params) })?;
        let made = (|| {
            let name = cstring("max_length")?;
            // SAFETY: live params handle and a valid C string.
            self.genai.check(unsafe { (a.generator_params_set_search_number)(params, name.as_ptr(), max_length as f64) })?;
            let mut generator: Handle = std::ptr::null_mut();
            // SAFETY: live model and params; the new handle is owned by the returned `Generator`.
            self.genai.check(unsafe { (a.create_generator)(self.handle, params, &mut generator) })?;
            let generator = Generator { genai: self.genai.clone(), handle: generator };
            // SAFETY: live generator and sequences handles.
            self.genai.check(unsafe { (a.generator_append_token_sequences)(generator.handle, prompt.handle) })?;
            Ok(generator)
        })();
        // SAFETY: created above, not used after this.
        unsafe { (a.destroy_generator_params)(params) };
        made
    }
}

pub struct Tokenizer {
    genai: Genai,
    handle: Handle,
}

impl Drop for Tokenizer {
    fn drop(&mut self) {
        // SAFETY: owned handle, dropped once.
        unsafe { (self.genai.0.destroy_tokenizer)(self.handle) }
    }
}

impl Tokenizer {
    pub fn encode(&self, text: &str) -> Result<Tokens, String> {
        let a = &self.genai.0;
        let mut seq: Handle = std::ptr::null_mut();
        // SAFETY: the new handle is owned by the returned `Tokens`.
        self.genai.check(unsafe { (a.create_sequences)(&mut seq) })?;
        let tokens = Tokens { genai: self.genai.clone(), handle: seq };
        let text = cstring(text)?;
        // SAFETY: live tokenizer and sequences handles, valid C string.
        self.genai.check(unsafe { (a.tokenizer_encode)(self.handle, text.as_ptr(), tokens.handle) })?;
        Ok(tokens)
    }

    pub fn stream(&self) -> Result<TokenizerStream, String> {
        let mut handle: Handle = std::ptr::null_mut();
        // SAFETY: live tokenizer; the new handle is owned by the returned stream.
        self.genai.check(unsafe { (self.genai.0.create_tokenizer_stream)(self.handle, &mut handle) })?;
        Ok(TokenizerStream { genai: self.genai.clone(), handle })
    }
}

/// Encoded text.
pub struct Tokens {
    genai: Genai,
    handle: Handle,
}

impl Drop for Tokens {
    fn drop(&mut self) {
        // SAFETY: owned handle, dropped once.
        unsafe { (self.genai.0.destroy_sequences)(self.handle) }
    }
}

impl Tokens {
    pub fn len(&self) -> usize {
        // SAFETY: live sequences handle; index 0 exists after one encode.
        unsafe { (self.genai.0.sequences_get_sequence_count)(self.handle, 0) }
    }
}

/// Turns tokens back into text piece by piece.
pub struct TokenizerStream {
    genai: Genai,
    handle: Handle,
}

impl Drop for TokenizerStream {
    fn drop(&mut self) {
        // SAFETY: owned handle, dropped once.
        unsafe { (self.genai.0.destroy_tokenizer_stream)(self.handle) }
    }
}

impl TokenizerStream {
    /// The text `token` adds (may be empty while a character is incomplete).
    pub fn decode(&mut self, token: i32) -> Result<String, String> {
        let mut out: *const c_char = std::ptr::null();
        // SAFETY: live stream handle; the returned string belongs to the stream and is copied
        // before the next call.
        self.genai.check(unsafe { (self.genai.0.tokenizer_stream_decode)(self.handle, token, &mut out) })?;
        if out.is_null() {
            return Ok(String::new());
        }
        // SAFETY: NUL-terminated string owned by the stream, valid until its next decode.
        Ok(unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned())
    }
}

pub struct Generator {
    genai: Genai,
    handle: Handle,
}

impl Drop for Generator {
    fn drop(&mut self) {
        // SAFETY: owned handle, dropped once.
        unsafe { (self.genai.0.destroy_generator)(self.handle) }
    }
}

impl Generator {
    pub fn is_done(&self) -> bool {
        // SAFETY: live generator handle.
        unsafe { (self.genai.0.generator_is_done)(self.handle) }
    }

    /// Generates the next token and returns it.
    pub fn next_token(&mut self) -> Result<Option<i32>, String> {
        let a = &self.genai.0;
        // SAFETY: live generator handle.
        self.genai.check(unsafe { (a.generator_generate_next_token)(self.handle) })?;
        let mut tokens: *const i32 = std::ptr::null();
        let mut count = 0usize;
        // SAFETY: as above; the array belongs to the generator and is read at once.
        self.genai.check(unsafe { (a.generator_get_next_tokens)(self.handle, &mut tokens, &mut count) })?;
        if tokens.is_null() || count == 0 {
            return Ok(None);
        }
        // SAFETY: `count` > 0 tokens at `tokens` (batch size 1: the first is ours).
        Ok(Some(unsafe { *tokens }))
    }
}
