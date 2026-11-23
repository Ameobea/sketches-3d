// This is ammo.js, a port of Bullet Physics to JavaScript. zlib licensed.
var Ammo = (() => {
  var _scriptName = typeof document != 'undefined' ? document.currentScript?.src : undefined;
  
  return (
async function(moduleArg = {}) {
  var moduleRtn;

// include: shell.js
// The Module object: Our interface to the outside world. We import
// and export values on it. There are various ways Module can be used:
// 1. Not defined. We create it here
// 2. A function parameter, function(moduleArg) => Promise<Module>
// 3. pre-run appended it, var Module = {}; ..generated code..
// 4. External script tag defines var Module.
// We need to check if Module already exists (e.g. case 3 above).
// Substitution will be replaced with actual code on later stage of the build,
// this way Closure Compiler will not mangle it (e.g. case 4. above).
// Note that if you want to run closure, and also to use Module
// after the generated code, you will need to define   var Module = {};
// before the code. Then that object will be used in the code, and you
// can continue to use Module afterwards as well.
var Module = moduleArg;

// Set up the promise that indicates the Module is initialized
var readyPromiseResolve, readyPromiseReject;

var readyPromise = new Promise((resolve, reject) => {
  readyPromiseResolve = resolve;
  readyPromiseReject = reject;
});

// Determine the runtime environment we are in. You can customize this by
// setting the ENVIRONMENT setting at compile time (see settings.js).
var ENVIRONMENT_IS_WEB = true;

var ENVIRONMENT_IS_WORKER = false;

// --pre-jses are emitted after the Module integration code, so that they can
// refer to Module (if they choose; they can also define Module)
// Sometimes an existing Module object exists with properties
// meant to overwrite the default module functionality. Here
// we collect those properties and reapply _after_ we configure
// the current environment's defaults to avoid having to be so
// defensive during initialization.
var moduleOverrides = {
  ...Module
};

var arguments_ = [];

var thisProgram = "./this.program";

var quit_ = (status, toThrow) => {
  throw toThrow;
};

// `/` should be present at the end if `scriptDirectory` is not empty
var scriptDirectory = "";

function locateFile(path) {
  if (Module["locateFile"]) {
    return Module["locateFile"](path, scriptDirectory);
  }
  return scriptDirectory + path;
}

// Hooks that are implemented differently in different runtime environments.
var readAsync, readBinary;

// Note that this includes Node.js workers when relevant (pthreads is enabled).
// Node.js workers are detected as a combination of ENVIRONMENT_IS_WORKER and
// ENVIRONMENT_IS_NODE.
if (ENVIRONMENT_IS_WEB || ENVIRONMENT_IS_WORKER) {
  if (ENVIRONMENT_IS_WORKER) {
    // Check worker, not web, since window could be polyfilled
    scriptDirectory = self.location.href;
  } else if (typeof document != "undefined" && document.currentScript) {
    // web
    scriptDirectory = document.currentScript.src;
  }
  // When MODULARIZE, this JS may be executed later, after document.currentScript
  // is gone, so we saved it, and we use it here instead of any other info.
  if (_scriptName) {
    scriptDirectory = _scriptName;
  }
  // blob urls look like blob:http://site.com/etc/etc and we cannot infer anything from them.
  // otherwise, slice off the final part of the url to find the script directory.
  // if scriptDirectory does not contain a slash, lastIndexOf will return -1,
  // and scriptDirectory will correctly be replaced with an empty string.
  // If scriptDirectory contains a query (starting with ?) or a fragment (starting with #),
  // they are removed because they could contain a slash.
  if (scriptDirectory.startsWith("blob:")) {
    scriptDirectory = "";
  } else {
    scriptDirectory = scriptDirectory.slice(0, scriptDirectory.replace(/[?#].*/, "").lastIndexOf("/") + 1);
  }
  {
    // include: web_or_worker_shell_read.js
    readAsync = async url => {
      var response = await fetch(url, {
        credentials: "same-origin"
      });
      if (response.ok) {
        return response.arrayBuffer();
      }
      throw new Error(response.status + " : " + response.url);
    };
  }
} else {}

var out = Module["print"] || console.log.bind(console);

var err = Module["printErr"] || console.error.bind(console);

// Merge back in the overrides
Object.assign(Module, moduleOverrides);

// Free the object hierarchy contained in the overrides, this lets the GC
// reclaim data used.
moduleOverrides = null;

// Emit code to handle expected values on the Module object. This applies Module.x
// to the proper local x. This has two benefits: first, we only emit it if it is
// expected to arrive, and second, by using a local everywhere else that can be
// minified.
if (Module["arguments"]) arguments_ = Module["arguments"];

if (Module["thisProgram"]) thisProgram = Module["thisProgram"];

// perform assertions in shell.js after we set up out() and err(), as otherwise if an assertion fails it cannot print the message
// end include: shell.js
// include: preamble.js
// === Preamble library stuff ===
// Documentation for the public APIs defined in this file must be updated in:
//    site/source/docs/api_reference/preamble.js.rst
// A prebuilt local version of the documentation is available at:
//    site/build/text/docs/api_reference/preamble.js.txt
// You can also build docs locally as HTML or other formats in site/
// An online HTML version (which may be of a different version of Emscripten)
//    is up at http://kripken.github.io/emscripten-site/docs/api_reference/preamble.js.html
var wasmBinary = Module["wasmBinary"];

// Wasm globals
var wasmMemory;

//========================================
// Runtime essentials
//========================================
// whether we are quitting the application. no code should run after this.
// set in exit() and abort()
var ABORT = false;

// set by exit() and abort().  Passed to 'onExit' handler.
// NOTE: This is also used as the process return code code in shell environments
// but only when noExitRuntime is false.
var EXITSTATUS;

// In STRICT mode, we only define assert() when ASSERTIONS is set.  i.e. we
// don't define it at all in release modes.  This matches the behaviour of
// MINIMAL_RUNTIME.
// TODO(sbc): Make this the default even without STRICT enabled.
/** @type {function(*, string=)} */ function assert(condition, text) {
  if (!condition) {
    // This build was created without ASSERTIONS defined.  `assert()` should not
    // ever be called in this configuration but in case there are callers in
    // the wild leave this simple abort() implementation here for now.
    abort(text);
  }
}

// Memory management
var /** @type {!Int8Array} */ HEAP8, /** @type {!Uint8Array} */ HEAPU8, /** @type {!Int16Array} */ HEAP16, /** @type {!Uint16Array} */ HEAPU16, /** @type {!Int32Array} */ HEAP32, /** @type {!Uint32Array} */ HEAPU32, /** @type {!Float32Array} */ HEAPF32, /* BigInt64Array type is not correctly defined in closure
/** not-@type {!BigInt64Array} */ HEAP64, /* BigUint64Array type is not correctly defined in closure
/** not-t@type {!BigUint64Array} */ HEAPU64, /** @type {!Float64Array} */ HEAPF64;

var runtimeInitialized = false;

// include: runtime_shared.js
// include: runtime_stack_check.js
// end include: runtime_stack_check.js
// include: runtime_exceptions.js
// end include: runtime_exceptions.js
// include: runtime_debug.js
// end include: runtime_debug.js
// include: memoryprofiler.js
// end include: memoryprofiler.js
function updateMemoryViews() {
  var b = wasmMemory.buffer;
  Module["HEAP8"] = HEAP8 = new Int8Array(b);
  Module["HEAP16"] = HEAP16 = new Int16Array(b);
  Module["HEAPU8"] = HEAPU8 = new Uint8Array(b);
  Module["HEAPU16"] = HEAPU16 = new Uint16Array(b);
  Module["HEAP32"] = HEAP32 = new Int32Array(b);
  Module["HEAPU32"] = HEAPU32 = new Uint32Array(b);
  Module["HEAPF32"] = HEAPF32 = new Float32Array(b);
  Module["HEAPF64"] = HEAPF64 = new Float64Array(b);
  Module["HEAP64"] = HEAP64 = new BigInt64Array(b);
  Module["HEAPU64"] = HEAPU64 = new BigUint64Array(b);
}

// end include: runtime_shared.js
function preRun() {
  if (Module["preRun"]) {
    if (typeof Module["preRun"] == "function") Module["preRun"] = [ Module["preRun"] ];
    while (Module["preRun"].length) {
      addOnPreRun(Module["preRun"].shift());
    }
  }
  callRuntimeCallbacks(onPreRuns);
}

function initRuntime() {
  runtimeInitialized = true;
  callRuntimeCallbacks(onInits);
  wasmExports["h"]();
}

function postRun() {
  if (Module["postRun"]) {
    if (typeof Module["postRun"] == "function") Module["postRun"] = [ Module["postRun"] ];
    while (Module["postRun"].length) {
      addOnPostRun(Module["postRun"].shift());
    }
  }
  callRuntimeCallbacks(onPostRuns);
}

// A counter of dependencies for calling run(). If we need to
// do asynchronous work before running, increment this and
// decrement it. Incrementing must happen in a place like
// Module.preRun (used by emcc to add file preloading).
// Note that you can add dependencies in preRun, even though
// it happens right before run - run will be postponed until
// the dependencies are met.
var runDependencies = 0;

var dependenciesFulfilled = null;

function addRunDependency(id) {
  runDependencies++;
  Module["monitorRunDependencies"]?.(runDependencies);
}

function removeRunDependency(id) {
  runDependencies--;
  Module["monitorRunDependencies"]?.(runDependencies);
  if (runDependencies == 0) {
    if (dependenciesFulfilled) {
      var callback = dependenciesFulfilled;
      dependenciesFulfilled = null;
      callback();
    }
  }
}

/** @param {string|number=} what */ function abort(what) {
  Module["onAbort"]?.(what);
  what = "Aborted(" + what + ")";
  // TODO(sbc): Should we remove printing and leave it up to whoever
  // catches the exception?
  err(what);
  ABORT = true;
  what += ". Build with -sASSERTIONS for more info.";
  // Use a wasm runtime error, because a JS error might be seen as a foreign
  // exception, which means we'd run destructors on it. We need the error to
  // simply make the program stop.
  // FIXME This approach does not work in Wasm EH because it currently does not assume
  // all RuntimeErrors are from traps; it decides whether a RuntimeError is from
  // a trap or not based on a hidden field within the object. So at the moment
  // we don't have a way of throwing a wasm trap from JS. TODO Make a JS API that
  // allows this in the wasm spec.
  // Suppress closure compiler warning here. Closure compiler's builtin extern
  // definition for WebAssembly.RuntimeError claims it takes no arguments even
  // though it can.
  // TODO(https://github.com/google/closure-compiler/pull/3913): Remove if/when upstream closure gets fixed.
  /** @suppress {checkTypes} */ var e = new WebAssembly.RuntimeError(what);
  readyPromiseReject(e);
  // Throw the error whether or not MODULARIZE is set because abort is used
  // in code paths apart from instantiation where an exception is expected
  // to be thrown when abort is called.
  throw e;
}

var wasmBinaryFile;

function findWasmBinary() {
  return locateFile("ammo.wasm.wasm");
}

function getBinarySync(file) {
  if (file == wasmBinaryFile && wasmBinary) {
    return new Uint8Array(wasmBinary);
  }
  if (readBinary) {
    return readBinary(file);
  }
  throw "both async and sync fetching of the wasm failed";
}

async function getWasmBinary(binaryFile) {
  // If we don't have the binary yet, load it asynchronously using readAsync.
  if (!wasmBinary) {
    // Fetch the binary using readAsync
    try {
      var response = await readAsync(binaryFile);
      return new Uint8Array(response);
    } catch {}
  }
  // Otherwise, getBinarySync should be able to get it synchronously
  return getBinarySync(binaryFile);
}

async function instantiateArrayBuffer(binaryFile, imports) {
  try {
    var binary = await getWasmBinary(binaryFile);
    var instance = await WebAssembly.instantiate(binary, imports);
    return instance;
  } catch (reason) {
    err(`failed to asynchronously prepare wasm: ${reason}`);
    abort(reason);
  }
}

async function instantiateAsync(binary, binaryFile, imports) {
  if (!binary && typeof WebAssembly.instantiateStreaming == "function") {
    try {
      var response = fetch(binaryFile, {
        credentials: "same-origin"
      });
      var instantiationResult = await WebAssembly.instantiateStreaming(response, imports);
      return instantiationResult;
    } catch (reason) {
      // We expect the most common failure cause to be a bad MIME type for the binary,
      // in which case falling back to ArrayBuffer instantiation should work.
      err(`wasm streaming compile failed: ${reason}`);
      err("falling back to ArrayBuffer instantiation");
    }
  }
  return instantiateArrayBuffer(binaryFile, imports);
}

function getWasmImports() {
  // prepare imports
  return {
    "a": wasmImports
  };
}

// Create the wasm instance.
// Receives the wasm imports, returns the exports.
async function createWasm() {
  // Load the wasm module and create an instance of using native support in the JS engine.
  // handle a generated wasm instance, receiving its exports and
  // performing other necessary setup
  /** @param {WebAssembly.Module=} module*/ function receiveInstance(instance, module) {
    wasmExports = instance.exports;
    wasmMemory = wasmExports["g"];
    updateMemoryViews();
    wasmTable = wasmExports["s"];
    removeRunDependency("wasm-instantiate");
    return wasmExports;
  }
  // wait for the pthread pool (if any)
  addRunDependency("wasm-instantiate");
  // Prefer streaming instantiation if available.
  function receiveInstantiationResult(result) {
    // 'result' is a ResultObject object which has both the module and instance.
    // receiveInstance() will swap in the exports (to Module.asm) so they can be called
    // TODO: Due to Closure regression https://github.com/google/closure-compiler/issues/3193, the above line no longer optimizes out down to the following line.
    // When the regression is fixed, can restore the above PTHREADS-enabled path.
    return receiveInstance(result["instance"]);
  }
  var info = getWasmImports();
  // User shell pages can write their own Module.instantiateWasm = function(imports, successCallback) callback
  // to manually instantiate the Wasm module themselves. This allows pages to
  // run the instantiation parallel to any other async startup actions they are
  // performing.
  // Also pthreads and wasm workers initialize the wasm instance through this
  // path.
  if (Module["instantiateWasm"]) {
    return new Promise((resolve, reject) => {
      Module["instantiateWasm"](info, (mod, inst) => {
        receiveInstance(mod, inst);
        resolve(mod.exports);
      });
    });
  }
  wasmBinaryFile ??= findWasmBinary();
  try {
    var result = await instantiateAsync(wasmBinary, wasmBinaryFile, info);
    var exports = receiveInstantiationResult(result);
    return exports;
  } catch (e) {
    // If instantiation fails, reject the module ready promise.
    readyPromiseReject(e);
    return Promise.reject(e);
  }
}

// end include: preamble.js
// Begin JS library code
class ExitStatus {
  name="ExitStatus";
  constructor(status) {
    this.message = `Program terminated with exit(${status})`;
    this.status = status;
  }
}

var callRuntimeCallbacks = callbacks => {
  while (callbacks.length > 0) {
    // Pass the module as the first argument.
    callbacks.shift()(Module);
  }
};

var onPostRuns = [];

var addOnPostRun = cb => onPostRuns.unshift(cb);

var onPreRuns = [];

var addOnPreRun = cb => onPreRuns.unshift(cb);

var noExitRuntime = Module["noExitRuntime"] || true;

var __abort_js = () => abort("");

var runtimeKeepaliveCounter = 0;

var __emscripten_runtime_keepalive_clear = () => {
  noExitRuntime = false;
  runtimeKeepaliveCounter = 0;
};

var timers = {};

var handleException = e => {
  // Certain exception types we do not treat as errors since they are used for
  // internal control flow.
  // 1. ExitStatus, which is thrown by exit()
  // 2. "unwind", which is thrown by emscripten_unwind_to_js_event_loop() and others
  //    that wish to return to JS event loop.
  if (e instanceof ExitStatus || e == "unwind") {
    return EXITSTATUS;
  }
  quit_(1, e);
};

var keepRuntimeAlive = () => noExitRuntime || runtimeKeepaliveCounter > 0;

var _proc_exit = code => {
  EXITSTATUS = code;
  if (!keepRuntimeAlive()) {
    Module["onExit"]?.(code);
    ABORT = true;
  }
  quit_(code, new ExitStatus(code));
};

/** @suppress {duplicate } */ /** @param {boolean|number=} implicit */ var exitJS = (status, implicit) => {
  EXITSTATUS = status;
  _proc_exit(status);
};

var _exit = exitJS;

var maybeExit = () => {
  if (!keepRuntimeAlive()) {
    try {
      _exit(EXITSTATUS);
    } catch (e) {
      handleException(e);
    }
  }
};

var callUserCallback = func => {
  if (ABORT) {
    return;
  }
  try {
    func();
    maybeExit();
  } catch (e) {
    handleException(e);
  }
};

var _emscripten_get_now = () => performance.now();

var __setitimer_js = (which, timeout_ms) => {
  // First, clear any existing timer.
  if (timers[which]) {
    clearTimeout(timers[which].id);
    delete timers[which];
  }
  // A timeout of zero simply cancels the current timeout so we have nothing
  // more to do.
  if (!timeout_ms) return 0;
  var id = setTimeout(() => {
    delete timers[which];
    callUserCallback(() => __emscripten_timeout(which, _emscripten_get_now()));
  }, timeout_ms);
  timers[which] = {
    id,
    timeout_ms
  };
  return 0;
};

var getHeapMax = () => // Stay one Wasm page short of 4GB: while e.g. Chrome is able to allocate
// full 4GB Wasm memories, the size will wrap back to 0 bytes in Wasm side
// for any code that deals with heap sizes, which would require special
// casing all heap size related code to treat 0 specially.
2147483648;

var alignMemory = (size, alignment) => Math.ceil(size / alignment) * alignment;

var growMemory = size => {
  var b = wasmMemory.buffer;
  var pages = ((size - b.byteLength + 65535) / 65536) | 0;
  try {
    // round size grow request up to wasm page size (fixed 64KB per spec)
    wasmMemory.grow(pages);
    // .grow() takes a delta compared to the previous size
    updateMemoryViews();
    return 1;
  } catch (e) {}
};

var _emscripten_resize_heap = requestedSize => {
  var oldSize = HEAPU8.length;
  // With CAN_ADDRESS_2GB or MEMORY64, pointers are already unsigned.
  requestedSize >>>= 0;
  // With multithreaded builds, races can happen (another thread might increase the size
  // in between), so return a failure, and let the caller retry.
  // Memory resize rules:
  // 1.  Always increase heap size to at least the requested size, rounded up
  //     to next page multiple.
  // 2a. If MEMORY_GROWTH_LINEAR_STEP == -1, excessively resize the heap
  //     geometrically: increase the heap size according to
  //     MEMORY_GROWTH_GEOMETRIC_STEP factor (default +20%), At most
  //     overreserve by MEMORY_GROWTH_GEOMETRIC_CAP bytes (default 96MB).
  // 2b. If MEMORY_GROWTH_LINEAR_STEP != -1, excessively resize the heap
  //     linearly: increase the heap size by at least
  //     MEMORY_GROWTH_LINEAR_STEP bytes.
  // 3.  Max size for the heap is capped at 2048MB-WASM_PAGE_SIZE, or by
  //     MAXIMUM_MEMORY, or by ASAN limit, depending on which is smallest
  // 4.  If we were unable to allocate as much memory, it may be due to
  //     over-eager decision to excessively reserve due to (3) above.
  //     Hence if an allocation fails, cut down on the amount of excess
  //     growth, in an attempt to succeed to perform a smaller allocation.
  // A limit is set for how much we can grow. We should not exceed that
  // (the wasm binary specifies it, so if we tried, we'd fail anyhow).
  var maxHeapSize = getHeapMax();
  if (requestedSize > maxHeapSize) {
    return false;
  }
  // Loop through potential heap size increases. If we attempt a too eager
  // reservation that fails, cut down on the attempted size and reserve a
  // smaller bump instead. (max 3 times, chosen somewhat arbitrarily)
  for (var cutDown = 1; cutDown <= 4; cutDown *= 2) {
    var overGrownHeapSize = oldSize * (1 + .2 / cutDown);
    // ensure geometric growth
    // but limit overreserving (default to capping at +96MB overgrowth at most)
    overGrownHeapSize = Math.min(overGrownHeapSize, requestedSize + 100663296);
    var newSize = Math.min(maxHeapSize, alignMemory(Math.max(requestedSize, overGrownHeapSize), 65536));
    var replacement = growMemory(newSize);
    if (replacement) {
      return true;
    }
  }
  return false;
};

var printCharBuffers = [ null, [], [] ];

var UTF8Decoder = typeof TextDecoder != "undefined" ? new TextDecoder : undefined;

/**
     * Given a pointer 'idx' to a null-terminated UTF8-encoded string in the given
     * array that contains uint8 values, returns a copy of that string as a
     * Javascript String object.
     * heapOrArray is either a regular array, or a JavaScript typed array view.
     * @param {number=} idx
     * @param {number=} maxBytesToRead
     * @return {string}
     */ var UTF8ArrayToString = (heapOrArray, idx = 0, maxBytesToRead = NaN) => {
  var endIdx = idx + maxBytesToRead;
  var endPtr = idx;
  // TextDecoder needs to know the byte length in advance, it doesn't stop on
  // null terminator by itself.  Also, use the length info to avoid running tiny
  // strings through TextDecoder, since .subarray() allocates garbage.
  // (As a tiny code save trick, compare endPtr against endIdx using a negation,
  // so that undefined/NaN means Infinity)
  while (heapOrArray[endPtr] && !(endPtr >= endIdx)) ++endPtr;
  if (endPtr - idx > 16 && heapOrArray.buffer && UTF8Decoder) {
    return UTF8Decoder.decode(heapOrArray.subarray(idx, endPtr));
  }
  var str = "";
  // If building with TextDecoder, we have already computed the string length
  // above, so test loop end condition against that
  while (idx < endPtr) {
    // For UTF8 byte structure, see:
    // http://en.wikipedia.org/wiki/UTF-8#Description
    // https://www.ietf.org/rfc/rfc2279.txt
    // https://tools.ietf.org/html/rfc3629
    var u0 = heapOrArray[idx++];
    if (!(u0 & 128)) {
      str += String.fromCharCode(u0);
      continue;
    }
    var u1 = heapOrArray[idx++] & 63;
    if ((u0 & 224) == 192) {
      str += String.fromCharCode(((u0 & 31) << 6) | u1);
      continue;
    }
    var u2 = heapOrArray[idx++] & 63;
    if ((u0 & 240) == 224) {
      u0 = ((u0 & 15) << 12) | (u1 << 6) | u2;
    } else {
      u0 = ((u0 & 7) << 18) | (u1 << 12) | (u2 << 6) | (heapOrArray[idx++] & 63);
    }
    if (u0 < 65536) {
      str += String.fromCharCode(u0);
    } else {
      var ch = u0 - 65536;
      str += String.fromCharCode(55296 | (ch >> 10), 56320 | (ch & 1023));
    }
  }
  return str;
};

var printChar = (stream, curr) => {
  var buffer = printCharBuffers[stream];
  if (curr === 0 || curr === 10) {
    (stream === 1 ? out : err)(UTF8ArrayToString(buffer));
    buffer.length = 0;
  } else {
    buffer.push(curr);
  }
};

/**
     * Given a pointer 'ptr' to a null-terminated UTF8-encoded string in the
     * emscripten HEAP, returns a copy of that string as a Javascript String object.
     *
     * @param {number} ptr
     * @param {number=} maxBytesToRead - An optional length that specifies the
     *   maximum number of bytes to read. You can omit this parameter to scan the
     *   string until the first 0 byte. If maxBytesToRead is passed, and the string
     *   at [ptr, ptr+maxBytesToReadr[ contains a null byte in the middle, then the
     *   string will cut short at that byte index (i.e. maxBytesToRead will not
     *   produce a string of exact length [ptr, ptr+maxBytesToRead[) N.B. mixing
     *   frequent uses of UTF8ToString() with and without maxBytesToRead may throw
     *   JS JIT optimizations off, so it is worth to consider consistently using one
     * @return {string}
     */ var UTF8ToString = (ptr, maxBytesToRead) => ptr ? UTF8ArrayToString(HEAPU8, ptr, maxBytesToRead) : "";

var _fd_write = (fd, iov, iovcnt, pnum) => {
  // hack to support printf in SYSCALLS_REQUIRE_FILESYSTEM=0
  var num = 0;
  for (var i = 0; i < iovcnt; i++) {
    var ptr = HEAPU32[((iov) >> 2)];
    var len = HEAPU32[(((iov) + (4)) >> 2)];
    iov += 8;
    for (var j = 0; j < len; j++) {
      printChar(fd, HEAPU8[ptr + j]);
    }
    num += len;
  }
  HEAPU32[((pnum) >> 2)] = num;
  return 0;
};

var onInits = [];

var addOnInit = cb => onInits.unshift(cb);

var uleb128Encode = (n, target) => {
  if (n < 128) {
    target.push(n);
  } else {
    target.push((n % 128) | 128, n >> 7);
  }
};

var sigToWasmTypes = sig => {
  var typeNames = {
    "i": "i32",
    "j": "i64",
    "f": "f32",
    "d": "f64",
    "e": "externref",
    "p": "i32"
  };
  var type = {
    parameters: [],
    results: sig[0] == "v" ? [] : [ typeNames[sig[0]] ]
  };
  for (var i = 1; i < sig.length; ++i) {
    type.parameters.push(typeNames[sig[i]]);
  }
  return type;
};

var generateFuncType = (sig, target) => {
  var sigRet = sig.slice(0, 1);
  var sigParam = sig.slice(1);
  var typeCodes = {
    "i": 127,
    // i32
    "p": 127,
    // i32
    "j": 126,
    // i64
    "f": 125,
    // f32
    "d": 124,
    // f64
    "e": 111
  };
  // Parameters, length + signatures
  target.push(96);
  uleb128Encode(sigParam.length, target);
  for (var paramType of sigParam) {
    target.push(typeCodes[paramType]);
  }
  // Return values, length + signatures
  // With no multi-return in MVP, either 0 (void) or 1 (anything else)
  if (sigRet == "v") {
    target.push(0);
  } else {
    target.push(1, typeCodes[sigRet]);
  }
};

var convertJsFunctionToWasm = (func, sig) => {
  // If the type reflection proposal is available, use the new
  // "WebAssembly.Function" constructor.
  // Otherwise, construct a minimal wasm module importing the JS function and
  // re-exporting it.
  if (typeof WebAssembly.Function == "function") {
    return new WebAssembly.Function(sigToWasmTypes(sig), func);
  }
  // The module is static, with the exception of the type section, which is
  // generated based on the signature passed in.
  var typeSectionBody = [ 1 ];
  generateFuncType(sig, typeSectionBody);
  // Rest of the module is static
  var bytes = [ 0, 97, 115, 109, // magic ("\0asm")
  1, 0, 0, 0, // version: 1
  1 ];
  // Write the overall length of the type section followed by the body
  uleb128Encode(typeSectionBody.length, bytes);
  bytes.push(...typeSectionBody);
  // The rest of the module is static
  bytes.push(2, 7, // import section
  // (import "e" "f" (func 0 (type 0)))
  1, 1, 101, 1, 102, 0, 0, 7, 5, // export section
  // (export "f" (func 0 (type 0)))
  1, 1, 102, 0, 0);
  // We can compile this wasm module synchronously because it is very small.
  // This accepts an import (at "e.f"), that it reroutes to an export (at "f")
  var module = new WebAssembly.Module(new Uint8Array(bytes));
  var instance = new WebAssembly.Instance(module, {
    "e": {
      "f": func
    }
  });
  var wrappedFunc = instance.exports["f"];
  return wrappedFunc;
};

var wasmTableMirror = [];

/** @type {WebAssembly.Table} */ var wasmTable;

var getWasmTableEntry = funcPtr => {
  var func = wasmTableMirror[funcPtr];
  if (!func) {
    /** @suppress {checkTypes} */ wasmTableMirror[funcPtr] = func = wasmTable.get(funcPtr);
  }
  return func;
};

var updateTableMap = (offset, count) => {
  if (functionsInTableMap) {
    for (var i = offset; i < offset + count; i++) {
      var item = getWasmTableEntry(i);
      // Ignore null values.
      if (item) {
        functionsInTableMap.set(item, i);
      }
    }
  }
};

var functionsInTableMap;

var getFunctionAddress = func => {
  // First, create the map if this is the first use.
  if (!functionsInTableMap) {
    functionsInTableMap = new WeakMap;
    updateTableMap(0, wasmTable.length);
  }
  return functionsInTableMap.get(func) || 0;
};

var freeTableIndexes = [];

var getEmptyTableSlot = () => {
  // Reuse a free index if there is one, otherwise grow.
  if (freeTableIndexes.length) {
    return freeTableIndexes.pop();
  }
  // Grow the table
  try {
    /** @suppress {checkTypes} */ wasmTable.grow(1);
  } catch (err) {
    if (!(err instanceof RangeError)) {
      throw err;
    }
    throw "Unable to grow wasm table. Set ALLOW_TABLE_GROWTH.";
  }
  return wasmTable.length - 1;
};

var setWasmTableEntry = (idx, func) => {
  /** @suppress {checkTypes} */ wasmTable.set(idx, func);
  // With ABORT_ON_WASM_EXCEPTIONS wasmTable.get is overridden to return wrapped
  // functions so we need to call it here to retrieve the potential wrapper correctly
  // instead of just storing 'func' directly into wasmTableMirror
  /** @suppress {checkTypes} */ wasmTableMirror[idx] = wasmTable.get(idx);
};

/** @param {string=} sig */ var addFunction = (func, sig) => {
  // Check if the function is already in the table, to ensure each function
  // gets a unique index.
  var rtn = getFunctionAddress(func);
  if (rtn) {
    return rtn;
  }
  // It's not in the table, add it now.
  var ret = getEmptyTableSlot();
  // Set the new value.
  try {
    // Attempting to call this with JS function will cause of table.set() to fail
    setWasmTableEntry(ret, func);
  } catch (err) {
    if (!(err instanceof TypeError)) {
      throw err;
    }
    var wrapped = convertJsFunctionToWasm(func, sig);
    setWasmTableEntry(ret, wrapped);
  }
  functionsInTableMap.set(func, ret);
  return ret;
};

// End JS library code
var wasmImports = {
  /** @export */ d: __abort_js,
  /** @export */ c: __emscripten_runtime_keepalive_clear,
  /** @export */ b: __setitimer_js,
  /** @export */ a: _emscripten_resize_heap,
  /** @export */ f: _fd_write,
  /** @export */ e: _proc_exit
};

var wasmExports = await createWasm();

var ___wasm_call_ctors = wasmExports["h"];

var _webidl_free = Module["_webidl_free"] = wasmExports["i"];

var _free = Module["_free"] = wasmExports["j"];

var _webidl_malloc = Module["_webidl_malloc"] = wasmExports["k"];

var _malloc = Module["_malloc"] = wasmExports["l"];

var _emscripten_bind_btCollisionObject_getCollisionShape_0 = Module["_emscripten_bind_btCollisionObject_getCollisionShape_0"] = wasmExports["m"];

var _emscripten_bind_btCollisionObject_setActivationState_1 = Module["_emscripten_bind_btCollisionObject_setActivationState_1"] = wasmExports["n"];

var _emscripten_bind_btCollisionObject_getWorldTransform_0 = Module["_emscripten_bind_btCollisionObject_getWorldTransform_0"] = wasmExports["o"];

var _emscripten_bind_btCollisionObject_setCollisionFlags_1 = Module["_emscripten_bind_btCollisionObject_setCollisionFlags_1"] = wasmExports["p"];

var _emscripten_bind_btCollisionObject_setWorldTransform_1 = Module["_emscripten_bind_btCollisionObject_setWorldTransform_1"] = wasmExports["q"];

var _emscripten_bind_btCollisionObject_setCollisionShape_1 = Module["_emscripten_bind_btCollisionObject_setCollisionShape_1"] = wasmExports["r"];

var _emscripten_bind_btCollisionObject_getUserIndex_0 = Module["_emscripten_bind_btCollisionObject_getUserIndex_0"] = wasmExports["t"];

var _emscripten_bind_btCollisionObject_setUserIndex_1 = Module["_emscripten_bind_btCollisionObject_setUserIndex_1"] = wasmExports["u"];

var _emscripten_bind_btCollisionObject_setUserIndex2_1 = Module["_emscripten_bind_btCollisionObject_setUserIndex2_1"] = wasmExports["v"];

var _emscripten_bind_btCollisionObject___destroy___0 = Module["_emscripten_bind_btCollisionObject___destroy___0"] = wasmExports["w"];

var _emscripten_bind_btCollisionShape_setLocalScaling_1 = Module["_emscripten_bind_btCollisionShape_setLocalScaling_1"] = wasmExports["x"];

var _emscripten_bind_btCollisionShape___destroy___0 = Module["_emscripten_bind_btCollisionShape___destroy___0"] = wasmExports["y"];

var _emscripten_bind_btCollisionWorld_getDispatcher_0 = Module["_emscripten_bind_btCollisionWorld_getDispatcher_0"] = wasmExports["z"];

var _emscripten_bind_btCollisionWorld_getDispatchInfo_0 = Module["_emscripten_bind_btCollisionWorld_getDispatchInfo_0"] = wasmExports["A"];

var _emscripten_bind_btCollisionWorld_addCollisionObject_1 = Module["_emscripten_bind_btCollisionWorld_addCollisionObject_1"] = wasmExports["B"];

var _emscripten_bind_btCollisionWorld_addCollisionObject_2 = Module["_emscripten_bind_btCollisionWorld_addCollisionObject_2"] = wasmExports["C"];

var _emscripten_bind_btCollisionWorld_addCollisionObject_3 = Module["_emscripten_bind_btCollisionWorld_addCollisionObject_3"] = wasmExports["D"];

var _emscripten_bind_btCollisionWorld_removeCollisionObject_1 = Module["_emscripten_bind_btCollisionWorld_removeCollisionObject_1"] = wasmExports["E"];

var _emscripten_bind_btCollisionWorld_getBroadphase_0 = Module["_emscripten_bind_btCollisionWorld_getBroadphase_0"] = wasmExports["F"];

var _emscripten_bind_btCollisionWorld_contactPairTestBinary_3 = Module["_emscripten_bind_btCollisionWorld_contactPairTestBinary_3"] = wasmExports["G"];

var _emscripten_bind_btCollisionWorld___destroy___0 = Module["_emscripten_bind_btCollisionWorld___destroy___0"] = wasmExports["H"];

var _emscripten_bind_btMotionState_getWorldTransform_1 = Module["_emscripten_bind_btMotionState_getWorldTransform_1"] = wasmExports["I"];

var _emscripten_bind_btMotionState_setWorldTransform_1 = Module["_emscripten_bind_btMotionState_setWorldTransform_1"] = wasmExports["J"];

var _emscripten_bind_btMotionState___destroy___0 = Module["_emscripten_bind_btMotionState___destroy___0"] = wasmExports["K"];

var _emscripten_bind_btStridingMeshInterface___destroy___0 = Module["_emscripten_bind_btStridingMeshInterface___destroy___0"] = wasmExports["L"];

var _emscripten_bind_btConcaveShape_setLocalScaling_1 = Module["_emscripten_bind_btConcaveShape_setLocalScaling_1"] = wasmExports["M"];

var _emscripten_bind_btConcaveShape___destroy___0 = Module["_emscripten_bind_btConcaveShape___destroy___0"] = wasmExports["N"];

var _emscripten_bind_btConstraintSolver___destroy___0 = Module["_emscripten_bind_btConstraintSolver___destroy___0"] = wasmExports["O"];

var _emscripten_bind_btDynamicsWorld_addAction_1 = Module["_emscripten_bind_btDynamicsWorld_addAction_1"] = wasmExports["P"];

var _emscripten_bind_btDynamicsWorld_getDispatcher_0 = Module["_emscripten_bind_btDynamicsWorld_getDispatcher_0"] = wasmExports["Q"];

var _emscripten_bind_btDynamicsWorld_getDispatchInfo_0 = Module["_emscripten_bind_btDynamicsWorld_getDispatchInfo_0"] = wasmExports["R"];

var _emscripten_bind_btDynamicsWorld_addCollisionObject_1 = Module["_emscripten_bind_btDynamicsWorld_addCollisionObject_1"] = wasmExports["S"];

var _emscripten_bind_btDynamicsWorld_addCollisionObject_2 = Module["_emscripten_bind_btDynamicsWorld_addCollisionObject_2"] = wasmExports["T"];

var _emscripten_bind_btDynamicsWorld_addCollisionObject_3 = Module["_emscripten_bind_btDynamicsWorld_addCollisionObject_3"] = wasmExports["U"];

var _emscripten_bind_btDynamicsWorld_removeCollisionObject_1 = Module["_emscripten_bind_btDynamicsWorld_removeCollisionObject_1"] = wasmExports["V"];

var _emscripten_bind_btDynamicsWorld_getBroadphase_0 = Module["_emscripten_bind_btDynamicsWorld_getBroadphase_0"] = wasmExports["W"];

var _emscripten_bind_btDynamicsWorld_contactPairTestBinary_3 = Module["_emscripten_bind_btDynamicsWorld_contactPairTestBinary_3"] = wasmExports["X"];

var _emscripten_bind_btDynamicsWorld___destroy___0 = Module["_emscripten_bind_btDynamicsWorld___destroy___0"] = wasmExports["Y"];

var _emscripten_bind_btActionInterface___destroy___0 = Module["_emscripten_bind_btActionInterface___destroy___0"] = wasmExports["Z"];

var _emscripten_bind_btGhostObject_btGhostObject_0 = Module["_emscripten_bind_btGhostObject_btGhostObject_0"] = wasmExports["_"];

var _emscripten_bind_btGhostObject_getNumOverlappingObjects_0 = Module["_emscripten_bind_btGhostObject_getNumOverlappingObjects_0"] = wasmExports["$"];

var _emscripten_bind_btGhostObject_getCollisionShape_0 = Module["_emscripten_bind_btGhostObject_getCollisionShape_0"] = wasmExports["aa"];

var _emscripten_bind_btGhostObject_setActivationState_1 = Module["_emscripten_bind_btGhostObject_setActivationState_1"] = wasmExports["ba"];

var _emscripten_bind_btGhostObject_getWorldTransform_0 = Module["_emscripten_bind_btGhostObject_getWorldTransform_0"] = wasmExports["ca"];

var _emscripten_bind_btGhostObject_setCollisionFlags_1 = Module["_emscripten_bind_btGhostObject_setCollisionFlags_1"] = wasmExports["da"];

var _emscripten_bind_btGhostObject_setWorldTransform_1 = Module["_emscripten_bind_btGhostObject_setWorldTransform_1"] = wasmExports["ea"];

var _emscripten_bind_btGhostObject_setCollisionShape_1 = Module["_emscripten_bind_btGhostObject_setCollisionShape_1"] = wasmExports["fa"];

var _emscripten_bind_btGhostObject_getUserIndex_0 = Module["_emscripten_bind_btGhostObject_getUserIndex_0"] = wasmExports["ga"];

var _emscripten_bind_btGhostObject_setUserIndex_1 = Module["_emscripten_bind_btGhostObject_setUserIndex_1"] = wasmExports["ha"];

var _emscripten_bind_btGhostObject_setUserIndex2_1 = Module["_emscripten_bind_btGhostObject_setUserIndex2_1"] = wasmExports["ia"];

var _emscripten_bind_btGhostObject___destroy___0 = Module["_emscripten_bind_btGhostObject___destroy___0"] = wasmExports["ja"];

var _emscripten_bind_VoidPtr___destroy___0 = Module["_emscripten_bind_VoidPtr___destroy___0"] = wasmExports["ka"];

var _emscripten_bind_btVector3_btVector3_0 = Module["_emscripten_bind_btVector3_btVector3_0"] = wasmExports["la"];

var _emscripten_bind_btVector3_btVector3_3 = Module["_emscripten_bind_btVector3_btVector3_3"] = wasmExports["ma"];

var _emscripten_bind_btVector3_length_0 = Module["_emscripten_bind_btVector3_length_0"] = wasmExports["na"];

var _emscripten_bind_btVector3_x_0 = Module["_emscripten_bind_btVector3_x_0"] = wasmExports["oa"];

var _emscripten_bind_btVector3_y_0 = Module["_emscripten_bind_btVector3_y_0"] = wasmExports["pa"];

var _emscripten_bind_btVector3_z_0 = Module["_emscripten_bind_btVector3_z_0"] = wasmExports["qa"];

var _emscripten_bind_btVector3_setValue_3 = Module["_emscripten_bind_btVector3_setValue_3"] = wasmExports["ra"];

var _emscripten_bind_btVector3___destroy___0 = Module["_emscripten_bind_btVector3___destroy___0"] = wasmExports["sa"];

var _emscripten_bind_btQuaternion_btQuaternion_4 = Module["_emscripten_bind_btQuaternion_btQuaternion_4"] = wasmExports["ta"];

var _emscripten_bind_btQuaternion_setValue_4 = Module["_emscripten_bind_btQuaternion_setValue_4"] = wasmExports["ua"];

var _emscripten_bind_btQuaternion_setEulerZYX_3 = Module["_emscripten_bind_btQuaternion_setEulerZYX_3"] = wasmExports["va"];

var _emscripten_bind_btQuaternion_x_0 = Module["_emscripten_bind_btQuaternion_x_0"] = wasmExports["wa"];

var _emscripten_bind_btQuaternion_y_0 = Module["_emscripten_bind_btQuaternion_y_0"] = wasmExports["xa"];

var _emscripten_bind_btQuaternion_z_0 = Module["_emscripten_bind_btQuaternion_z_0"] = wasmExports["ya"];

var _emscripten_bind_btQuaternion_w_0 = Module["_emscripten_bind_btQuaternion_w_0"] = wasmExports["za"];

var _emscripten_bind_btQuaternion___destroy___0 = Module["_emscripten_bind_btQuaternion___destroy___0"] = wasmExports["Aa"];

var _emscripten_bind_btTransform_btTransform_0 = Module["_emscripten_bind_btTransform_btTransform_0"] = wasmExports["Ba"];

var _emscripten_bind_btTransform_btTransform_2 = Module["_emscripten_bind_btTransform_btTransform_2"] = wasmExports["Ca"];

var _emscripten_bind_btTransform_setIdentity_0 = Module["_emscripten_bind_btTransform_setIdentity_0"] = wasmExports["Da"];

var _emscripten_bind_btTransform_setOrigin_1 = Module["_emscripten_bind_btTransform_setOrigin_1"] = wasmExports["Ea"];

var _emscripten_bind_btTransform_setRotation_1 = Module["_emscripten_bind_btTransform_setRotation_1"] = wasmExports["Fa"];

var _emscripten_bind_btTransform_getOrigin_0 = Module["_emscripten_bind_btTransform_getOrigin_0"] = wasmExports["Ga"];

var _emscripten_bind_btTransform_setEulerZYX_3 = Module["_emscripten_bind_btTransform_setEulerZYX_3"] = wasmExports["Ha"];

var _emscripten_bind_btTransform_getRotation_0 = Module["_emscripten_bind_btTransform_getRotation_0"] = wasmExports["Ia"];

var _emscripten_bind_btTransform___destroy___0 = Module["_emscripten_bind_btTransform___destroy___0"] = wasmExports["Ja"];

var _emscripten_bind_btDefaultMotionState_btDefaultMotionState_0 = Module["_emscripten_bind_btDefaultMotionState_btDefaultMotionState_0"] = wasmExports["Ka"];

var _emscripten_bind_btDefaultMotionState_btDefaultMotionState_1 = Module["_emscripten_bind_btDefaultMotionState_btDefaultMotionState_1"] = wasmExports["La"];

var _emscripten_bind_btDefaultMotionState_btDefaultMotionState_2 = Module["_emscripten_bind_btDefaultMotionState_btDefaultMotionState_2"] = wasmExports["Ma"];

var _emscripten_bind_btDefaultMotionState_getWorldTransform_1 = Module["_emscripten_bind_btDefaultMotionState_getWorldTransform_1"] = wasmExports["Na"];

var _emscripten_bind_btDefaultMotionState_setWorldTransform_1 = Module["_emscripten_bind_btDefaultMotionState_setWorldTransform_1"] = wasmExports["Oa"];

var _emscripten_bind_btDefaultMotionState___destroy___0 = Module["_emscripten_bind_btDefaultMotionState___destroy___0"] = wasmExports["Pa"];

var _emscripten_bind_btConvexShape_setLocalScaling_1 = Module["_emscripten_bind_btConvexShape_setLocalScaling_1"] = wasmExports["Qa"];

var _emscripten_bind_btConvexShape___destroy___0 = Module["_emscripten_bind_btConvexShape___destroy___0"] = wasmExports["Ra"];

var _emscripten_bind_btBoxShape_btBoxShape_1 = Module["_emscripten_bind_btBoxShape_btBoxShape_1"] = wasmExports["Sa"];

var _emscripten_bind_btBoxShape_setLocalScaling_1 = Module["_emscripten_bind_btBoxShape_setLocalScaling_1"] = wasmExports["Ta"];

var _emscripten_bind_btBoxShape___destroy___0 = Module["_emscripten_bind_btBoxShape___destroy___0"] = wasmExports["Ua"];

var _emscripten_bind_btCapsuleShape_btCapsuleShape_2 = Module["_emscripten_bind_btCapsuleShape_btCapsuleShape_2"] = wasmExports["Va"];

var _emscripten_bind_btCapsuleShape_setLocalScaling_1 = Module["_emscripten_bind_btCapsuleShape_setLocalScaling_1"] = wasmExports["Wa"];

var _emscripten_bind_btCapsuleShape___destroy___0 = Module["_emscripten_bind_btCapsuleShape___destroy___0"] = wasmExports["Xa"];

var _emscripten_bind_btCylinderShape_btCylinderShape_1 = Module["_emscripten_bind_btCylinderShape_btCylinderShape_1"] = wasmExports["Ya"];

var _emscripten_bind_btCylinderShape_setLocalScaling_1 = Module["_emscripten_bind_btCylinderShape_setLocalScaling_1"] = wasmExports["Za"];

var _emscripten_bind_btCylinderShape___destroy___0 = Module["_emscripten_bind_btCylinderShape___destroy___0"] = wasmExports["_a"];

var _emscripten_bind_btSphereShape_btSphereShape_1 = Module["_emscripten_bind_btSphereShape_btSphereShape_1"] = wasmExports["$a"];

var _emscripten_bind_btSphereShape_setLocalScaling_1 = Module["_emscripten_bind_btSphereShape_setLocalScaling_1"] = wasmExports["ab"];

var _emscripten_bind_btSphereShape___destroy___0 = Module["_emscripten_bind_btSphereShape___destroy___0"] = wasmExports["bb"];

var _emscripten_bind_btConeShape_btConeShape_2 = Module["_emscripten_bind_btConeShape_btConeShape_2"] = wasmExports["cb"];

var _emscripten_bind_btConeShape_setLocalScaling_1 = Module["_emscripten_bind_btConeShape_setLocalScaling_1"] = wasmExports["db"];

var _emscripten_bind_btConeShape___destroy___0 = Module["_emscripten_bind_btConeShape___destroy___0"] = wasmExports["eb"];

var _emscripten_bind_btConvexHullShape_btConvexHullShape_0 = Module["_emscripten_bind_btConvexHullShape_btConvexHullShape_0"] = wasmExports["fb"];

var _emscripten_bind_btConvexHullShape_btConvexHullShape_1 = Module["_emscripten_bind_btConvexHullShape_btConvexHullShape_1"] = wasmExports["gb"];

var _emscripten_bind_btConvexHullShape_btConvexHullShape_2 = Module["_emscripten_bind_btConvexHullShape_btConvexHullShape_2"] = wasmExports["hb"];

var _emscripten_bind_btConvexHullShape_addPoint_1 = Module["_emscripten_bind_btConvexHullShape_addPoint_1"] = wasmExports["ib"];

var _emscripten_bind_btConvexHullShape_addPoint_2 = Module["_emscripten_bind_btConvexHullShape_addPoint_2"] = wasmExports["jb"];

var _emscripten_bind_btConvexHullShape_setLocalScaling_1 = Module["_emscripten_bind_btConvexHullShape_setLocalScaling_1"] = wasmExports["kb"];

var _emscripten_bind_btConvexHullShape___destroy___0 = Module["_emscripten_bind_btConvexHullShape___destroy___0"] = wasmExports["lb"];

var _emscripten_bind_btCompoundShape_btCompoundShape_0 = Module["_emscripten_bind_btCompoundShape_btCompoundShape_0"] = wasmExports["mb"];

var _emscripten_bind_btCompoundShape_btCompoundShape_1 = Module["_emscripten_bind_btCompoundShape_btCompoundShape_1"] = wasmExports["nb"];

var _emscripten_bind_btCompoundShape_addChildShape_2 = Module["_emscripten_bind_btCompoundShape_addChildShape_2"] = wasmExports["ob"];

var _emscripten_bind_btCompoundShape_setLocalScaling_1 = Module["_emscripten_bind_btCompoundShape_setLocalScaling_1"] = wasmExports["pb"];

var _emscripten_bind_btCompoundShape___destroy___0 = Module["_emscripten_bind_btCompoundShape___destroy___0"] = wasmExports["qb"];

var _emscripten_bind_btTriangleMesh_btTriangleMesh_0 = Module["_emscripten_bind_btTriangleMesh_btTriangleMesh_0"] = wasmExports["rb"];

var _emscripten_bind_btTriangleMesh_btTriangleMesh_1 = Module["_emscripten_bind_btTriangleMesh_btTriangleMesh_1"] = wasmExports["sb"];

var _emscripten_bind_btTriangleMesh_btTriangleMesh_2 = Module["_emscripten_bind_btTriangleMesh_btTriangleMesh_2"] = wasmExports["tb"];

var _emscripten_bind_btTriangleMesh_addTriangle_3 = Module["_emscripten_bind_btTriangleMesh_addTriangle_3"] = wasmExports["ub"];

var _emscripten_bind_btTriangleMesh_addTriangle_4 = Module["_emscripten_bind_btTriangleMesh_addTriangle_4"] = wasmExports["vb"];

var _emscripten_bind_btTriangleMesh_preallocateIndices_1 = Module["_emscripten_bind_btTriangleMesh_preallocateIndices_1"] = wasmExports["wb"];

var _emscripten_bind_btTriangleMesh_preallocateVertices_1 = Module["_emscripten_bind_btTriangleMesh_preallocateVertices_1"] = wasmExports["xb"];

var _emscripten_bind_btTriangleMesh___destroy___0 = Module["_emscripten_bind_btTriangleMesh___destroy___0"] = wasmExports["yb"];

var _emscripten_bind_btTriangleIndexVertexArrayWrapper_btTriangleIndexVertexArrayWrapper_6 = Module["_emscripten_bind_btTriangleIndexVertexArrayWrapper_btTriangleIndexVertexArrayWrapper_6"] = wasmExports["zb"];

var _emscripten_bind_btTriangleIndexVertexArrayWrapper___destroy___0 = Module["_emscripten_bind_btTriangleIndexVertexArrayWrapper___destroy___0"] = wasmExports["Ab"];

var _emscripten_bind_btTriangleInfoMap_btTriangleInfoMap_0 = Module["_emscripten_bind_btTriangleInfoMap_btTriangleInfoMap_0"] = wasmExports["Bb"];

var _emscripten_bind_btTriangleInfoMap_get_m_convexEpsilon_0 = Module["_emscripten_bind_btTriangleInfoMap_get_m_convexEpsilon_0"] = wasmExports["Cb"];

var _emscripten_bind_btTriangleInfoMap_set_m_convexEpsilon_1 = Module["_emscripten_bind_btTriangleInfoMap_set_m_convexEpsilon_1"] = wasmExports["Db"];

var _emscripten_bind_btTriangleInfoMap_get_m_planarEpsilon_0 = Module["_emscripten_bind_btTriangleInfoMap_get_m_planarEpsilon_0"] = wasmExports["Eb"];

var _emscripten_bind_btTriangleInfoMap_set_m_planarEpsilon_1 = Module["_emscripten_bind_btTriangleInfoMap_set_m_planarEpsilon_1"] = wasmExports["Fb"];

var _emscripten_bind_btTriangleInfoMap_get_m_equalVertexThreshold_0 = Module["_emscripten_bind_btTriangleInfoMap_get_m_equalVertexThreshold_0"] = wasmExports["Gb"];

var _emscripten_bind_btTriangleInfoMap_set_m_equalVertexThreshold_1 = Module["_emscripten_bind_btTriangleInfoMap_set_m_equalVertexThreshold_1"] = wasmExports["Hb"];

var _emscripten_bind_btTriangleInfoMap_get_m_edgeDistanceThreshold_0 = Module["_emscripten_bind_btTriangleInfoMap_get_m_edgeDistanceThreshold_0"] = wasmExports["Ib"];

var _emscripten_bind_btTriangleInfoMap_set_m_edgeDistanceThreshold_1 = Module["_emscripten_bind_btTriangleInfoMap_set_m_edgeDistanceThreshold_1"] = wasmExports["Jb"];

var _emscripten_bind_btTriangleInfoMap_get_m_maxEdgeAngleThreshold_0 = Module["_emscripten_bind_btTriangleInfoMap_get_m_maxEdgeAngleThreshold_0"] = wasmExports["Kb"];

var _emscripten_bind_btTriangleInfoMap_set_m_maxEdgeAngleThreshold_1 = Module["_emscripten_bind_btTriangleInfoMap_set_m_maxEdgeAngleThreshold_1"] = wasmExports["Lb"];

var _emscripten_bind_btTriangleInfoMap_get_m_zeroAreaThreshold_0 = Module["_emscripten_bind_btTriangleInfoMap_get_m_zeroAreaThreshold_0"] = wasmExports["Mb"];

var _emscripten_bind_btTriangleInfoMap_set_m_zeroAreaThreshold_1 = Module["_emscripten_bind_btTriangleInfoMap_set_m_zeroAreaThreshold_1"] = wasmExports["Nb"];

var _emscripten_bind_btTriangleInfoMap___destroy___0 = Module["_emscripten_bind_btTriangleInfoMap___destroy___0"] = wasmExports["Ob"];

var _emscripten_bind_btBvhTriangleMeshShape_btBvhTriangleMeshShape_2 = Module["_emscripten_bind_btBvhTriangleMeshShape_btBvhTriangleMeshShape_2"] = wasmExports["Pb"];

var _emscripten_bind_btBvhTriangleMeshShape_btBvhTriangleMeshShape_3 = Module["_emscripten_bind_btBvhTriangleMeshShape_btBvhTriangleMeshShape_3"] = wasmExports["Qb"];

var _emscripten_bind_btBvhTriangleMeshShape_setTriangleInfoMap_1 = Module["_emscripten_bind_btBvhTriangleMeshShape_setTriangleInfoMap_1"] = wasmExports["Rb"];

var _emscripten_bind_btBvhTriangleMeshShape_getTriangleInfoMap_0 = Module["_emscripten_bind_btBvhTriangleMeshShape_getTriangleInfoMap_0"] = wasmExports["Sb"];

var _emscripten_bind_btBvhTriangleMeshShape___destroy___0 = Module["_emscripten_bind_btBvhTriangleMeshShape___destroy___0"] = wasmExports["Tb"];

var _emscripten_bind_btScaledBvhTriangleMeshShape_btScaledBvhTriangleMeshShape_2 = Module["_emscripten_bind_btScaledBvhTriangleMeshShape_btScaledBvhTriangleMeshShape_2"] = wasmExports["Ub"];

var _emscripten_bind_btScaledBvhTriangleMeshShape_getChildShape_0 = Module["_emscripten_bind_btScaledBvhTriangleMeshShape_getChildShape_0"] = wasmExports["Vb"];

var _emscripten_bind_btScaledBvhTriangleMeshShape_setLocalScaling_1 = Module["_emscripten_bind_btScaledBvhTriangleMeshShape_setLocalScaling_1"] = wasmExports["Wb"];

var _emscripten_bind_btScaledBvhTriangleMeshShape___destroy___0 = Module["_emscripten_bind_btScaledBvhTriangleMeshShape___destroy___0"] = wasmExports["Xb"];

var _emscripten_bind_btInternalEdgeUtility_btGenerateInternalEdgeInfo_2 = Module["_emscripten_bind_btInternalEdgeUtility_btGenerateInternalEdgeInfo_2"] = wasmExports["Yb"];

var _emscripten_bind_btInternalEdgeUtility___destroy___0 = Module["_emscripten_bind_btInternalEdgeUtility___destroy___0"] = wasmExports["Zb"];

var _emscripten_bind_btHeightfieldTerrainShape_btHeightfieldTerrainShape_9 = Module["_emscripten_bind_btHeightfieldTerrainShape_btHeightfieldTerrainShape_9"] = wasmExports["_b"];

var _emscripten_bind_btHeightfieldTerrainShape_setMargin_1 = Module["_emscripten_bind_btHeightfieldTerrainShape_setMargin_1"] = wasmExports["$b"];

var _emscripten_bind_btHeightfieldTerrainShape_getMargin_0 = Module["_emscripten_bind_btHeightfieldTerrainShape_getMargin_0"] = wasmExports["ac"];

var _emscripten_bind_btHeightfieldTerrainShape_setLocalScaling_1 = Module["_emscripten_bind_btHeightfieldTerrainShape_setLocalScaling_1"] = wasmExports["bc"];

var _emscripten_bind_btHeightfieldTerrainShape___destroy___0 = Module["_emscripten_bind_btHeightfieldTerrainShape___destroy___0"] = wasmExports["cc"];

var _emscripten_bind_btDefaultCollisionConstructionInfo_btDefaultCollisionConstructionInfo_0 = Module["_emscripten_bind_btDefaultCollisionConstructionInfo_btDefaultCollisionConstructionInfo_0"] = wasmExports["dc"];

var _emscripten_bind_btDefaultCollisionConstructionInfo___destroy___0 = Module["_emscripten_bind_btDefaultCollisionConstructionInfo___destroy___0"] = wasmExports["ec"];

var _emscripten_bind_btDefaultCollisionConfiguration_btDefaultCollisionConfiguration_0 = Module["_emscripten_bind_btDefaultCollisionConfiguration_btDefaultCollisionConfiguration_0"] = wasmExports["fc"];

var _emscripten_bind_btDefaultCollisionConfiguration_btDefaultCollisionConfiguration_1 = Module["_emscripten_bind_btDefaultCollisionConfiguration_btDefaultCollisionConfiguration_1"] = wasmExports["gc"];

var _emscripten_bind_btDefaultCollisionConfiguration___destroy___0 = Module["_emscripten_bind_btDefaultCollisionConfiguration___destroy___0"] = wasmExports["hc"];

var _emscripten_bind_btDispatcher___destroy___0 = Module["_emscripten_bind_btDispatcher___destroy___0"] = wasmExports["ic"];

var _emscripten_bind_btCollisionDispatcher_btCollisionDispatcher_1 = Module["_emscripten_bind_btCollisionDispatcher_btCollisionDispatcher_1"] = wasmExports["jc"];

var _emscripten_bind_btCollisionDispatcher___destroy___0 = Module["_emscripten_bind_btCollisionDispatcher___destroy___0"] = wasmExports["kc"];

var _emscripten_bind_btOverlappingPairCallback___destroy___0 = Module["_emscripten_bind_btOverlappingPairCallback___destroy___0"] = wasmExports["lc"];

var _emscripten_bind_btOverlappingPairCache_setInternalGhostPairCallback_1 = Module["_emscripten_bind_btOverlappingPairCache_setInternalGhostPairCallback_1"] = wasmExports["mc"];

var _emscripten_bind_btOverlappingPairCache___destroy___0 = Module["_emscripten_bind_btOverlappingPairCache___destroy___0"] = wasmExports["nc"];

var _emscripten_bind_btBroadphaseInterface_getOverlappingPairCache_0 = Module["_emscripten_bind_btBroadphaseInterface_getOverlappingPairCache_0"] = wasmExports["oc"];

var _emscripten_bind_btBroadphaseInterface___destroy___0 = Module["_emscripten_bind_btBroadphaseInterface___destroy___0"] = wasmExports["pc"];

var _emscripten_bind_btCollisionConfiguration___destroy___0 = Module["_emscripten_bind_btCollisionConfiguration___destroy___0"] = wasmExports["qc"];

var _emscripten_bind_btDbvtBroadphase_btDbvtBroadphase_0 = Module["_emscripten_bind_btDbvtBroadphase_btDbvtBroadphase_0"] = wasmExports["rc"];

var _emscripten_bind_btDbvtBroadphase_optimize_0 = Module["_emscripten_bind_btDbvtBroadphase_optimize_0"] = wasmExports["sc"];

var _emscripten_bind_btDbvtBroadphase___destroy___0 = Module["_emscripten_bind_btDbvtBroadphase___destroy___0"] = wasmExports["tc"];

var _emscripten_bind_btRigidBodyConstructionInfo_btRigidBodyConstructionInfo_3 = Module["_emscripten_bind_btRigidBodyConstructionInfo_btRigidBodyConstructionInfo_3"] = wasmExports["uc"];

var _emscripten_bind_btRigidBodyConstructionInfo_btRigidBodyConstructionInfo_4 = Module["_emscripten_bind_btRigidBodyConstructionInfo_btRigidBodyConstructionInfo_4"] = wasmExports["vc"];

var _emscripten_bind_btRigidBodyConstructionInfo___destroy___0 = Module["_emscripten_bind_btRigidBodyConstructionInfo___destroy___0"] = wasmExports["wc"];

var _emscripten_bind_btRigidBody_btRigidBody_1 = Module["_emscripten_bind_btRigidBody_btRigidBody_1"] = wasmExports["xc"];

var _emscripten_bind_btRigidBody_getMotionState_0 = Module["_emscripten_bind_btRigidBody_getMotionState_0"] = wasmExports["yc"];

var _emscripten_bind_btRigidBody_upcast_1 = Module["_emscripten_bind_btRigidBody_upcast_1"] = wasmExports["zc"];

var _emscripten_bind_btRigidBody_getCollisionShape_0 = Module["_emscripten_bind_btRigidBody_getCollisionShape_0"] = wasmExports["Ac"];

var _emscripten_bind_btRigidBody_setActivationState_1 = Module["_emscripten_bind_btRigidBody_setActivationState_1"] = wasmExports["Bc"];

var _emscripten_bind_btRigidBody_getWorldTransform_0 = Module["_emscripten_bind_btRigidBody_getWorldTransform_0"] = wasmExports["Cc"];

var _emscripten_bind_btRigidBody_setCollisionFlags_1 = Module["_emscripten_bind_btRigidBody_setCollisionFlags_1"] = wasmExports["Dc"];

var _emscripten_bind_btRigidBody_setWorldTransform_1 = Module["_emscripten_bind_btRigidBody_setWorldTransform_1"] = wasmExports["Ec"];

var _emscripten_bind_btRigidBody_setCollisionShape_1 = Module["_emscripten_bind_btRigidBody_setCollisionShape_1"] = wasmExports["Fc"];

var _emscripten_bind_btRigidBody_getUserIndex_0 = Module["_emscripten_bind_btRigidBody_getUserIndex_0"] = wasmExports["Gc"];

var _emscripten_bind_btRigidBody_setUserIndex_1 = Module["_emscripten_bind_btRigidBody_setUserIndex_1"] = wasmExports["Hc"];

var _emscripten_bind_btRigidBody_setUserIndex2_1 = Module["_emscripten_bind_btRigidBody_setUserIndex2_1"] = wasmExports["Ic"];

var _emscripten_bind_btRigidBody___destroy___0 = Module["_emscripten_bind_btRigidBody___destroy___0"] = wasmExports["Jc"];

var _emscripten_bind_btSequentialImpulseConstraintSolver_btSequentialImpulseConstraintSolver_0 = Module["_emscripten_bind_btSequentialImpulseConstraintSolver_btSequentialImpulseConstraintSolver_0"] = wasmExports["Kc"];

var _emscripten_bind_btSequentialImpulseConstraintSolver___destroy___0 = Module["_emscripten_bind_btSequentialImpulseConstraintSolver___destroy___0"] = wasmExports["Lc"];

var _emscripten_bind_btDispatcherInfo_get_m_useContinuous_0 = Module["_emscripten_bind_btDispatcherInfo_get_m_useContinuous_0"] = wasmExports["Mc"];

var _emscripten_bind_btDispatcherInfo_set_m_useContinuous_1 = Module["_emscripten_bind_btDispatcherInfo_set_m_useContinuous_1"] = wasmExports["Nc"];

var _emscripten_bind_btDispatcherInfo___destroy___0 = Module["_emscripten_bind_btDispatcherInfo___destroy___0"] = wasmExports["Oc"];

var _emscripten_bind_btDiscreteDynamicsWorld_btDiscreteDynamicsWorld_4 = Module["_emscripten_bind_btDiscreteDynamicsWorld_btDiscreteDynamicsWorld_4"] = wasmExports["Pc"];

var _emscripten_bind_btDiscreteDynamicsWorld_setGravity_1 = Module["_emscripten_bind_btDiscreteDynamicsWorld_setGravity_1"] = wasmExports["Qc"];

var _emscripten_bind_btDiscreteDynamicsWorld_addRigidBody_1 = Module["_emscripten_bind_btDiscreteDynamicsWorld_addRigidBody_1"] = wasmExports["Rc"];

var _emscripten_bind_btDiscreteDynamicsWorld_addRigidBody_3 = Module["_emscripten_bind_btDiscreteDynamicsWorld_addRigidBody_3"] = wasmExports["Sc"];

var _emscripten_bind_btDiscreteDynamicsWorld_removeRigidBody_1 = Module["_emscripten_bind_btDiscreteDynamicsWorld_removeRigidBody_1"] = wasmExports["Tc"];

var _emscripten_bind_btDiscreteDynamicsWorld_stepSimulation_1 = Module["_emscripten_bind_btDiscreteDynamicsWorld_stepSimulation_1"] = wasmExports["Uc"];

var _emscripten_bind_btDiscreteDynamicsWorld_stepSimulation_2 = Module["_emscripten_bind_btDiscreteDynamicsWorld_stepSimulation_2"] = wasmExports["Vc"];

var _emscripten_bind_btDiscreteDynamicsWorld_stepSimulation_3 = Module["_emscripten_bind_btDiscreteDynamicsWorld_stepSimulation_3"] = wasmExports["Wc"];

var _emscripten_bind_btDiscreteDynamicsWorld_substepSimulation_1 = Module["_emscripten_bind_btDiscreteDynamicsWorld_substepSimulation_1"] = wasmExports["Xc"];

var _emscripten_bind_btDiscreteDynamicsWorld_getDispatcher_0 = Module["_emscripten_bind_btDiscreteDynamicsWorld_getDispatcher_0"] = wasmExports["Yc"];

var _emscripten_bind_btDiscreteDynamicsWorld_getDispatchInfo_0 = Module["_emscripten_bind_btDiscreteDynamicsWorld_getDispatchInfo_0"] = wasmExports["Zc"];

var _emscripten_bind_btDiscreteDynamicsWorld_addCollisionObject_1 = Module["_emscripten_bind_btDiscreteDynamicsWorld_addCollisionObject_1"] = wasmExports["_c"];

var _emscripten_bind_btDiscreteDynamicsWorld_addCollisionObject_2 = Module["_emscripten_bind_btDiscreteDynamicsWorld_addCollisionObject_2"] = wasmExports["$c"];

var _emscripten_bind_btDiscreteDynamicsWorld_addCollisionObject_3 = Module["_emscripten_bind_btDiscreteDynamicsWorld_addCollisionObject_3"] = wasmExports["ad"];

var _emscripten_bind_btDiscreteDynamicsWorld_removeCollisionObject_1 = Module["_emscripten_bind_btDiscreteDynamicsWorld_removeCollisionObject_1"] = wasmExports["bd"];

var _emscripten_bind_btDiscreteDynamicsWorld_getBroadphase_0 = Module["_emscripten_bind_btDiscreteDynamicsWorld_getBroadphase_0"] = wasmExports["cd"];

var _emscripten_bind_btDiscreteDynamicsWorld_contactPairTestBinary_3 = Module["_emscripten_bind_btDiscreteDynamicsWorld_contactPairTestBinary_3"] = wasmExports["dd"];

var _emscripten_bind_btDiscreteDynamicsWorld_addAction_1 = Module["_emscripten_bind_btDiscreteDynamicsWorld_addAction_1"] = wasmExports["ed"];

var _emscripten_bind_btDiscreteDynamicsWorld___destroy___0 = Module["_emscripten_bind_btDiscreteDynamicsWorld___destroy___0"] = wasmExports["fd"];

var _emscripten_bind_btJumpPad_btJumpPad_5 = Module["_emscripten_bind_btJumpPad_btJumpPad_5"] = wasmExports["gd"];

var _emscripten_bind_btJumpPad_setDirection_1 = Module["_emscripten_bind_btJumpPad_setDirection_1"] = wasmExports["hd"];

var _emscripten_bind_btJumpPad_setBaseImpulse_1 = Module["_emscripten_bind_btJumpPad_setBaseImpulse_1"] = wasmExports["id"];

var _emscripten_bind_btJumpPad_setSpeedScaling_1 = Module["_emscripten_bind_btJumpPad_setSpeedScaling_1"] = wasmExports["jd"];

var _emscripten_bind_btJumpPad_setCooldownSeconds_1 = Module["_emscripten_bind_btJumpPad_setCooldownSeconds_1"] = wasmExports["kd"];

var _emscripten_bind_btJumpPad_getZoneId_0 = Module["_emscripten_bind_btJumpPad_getZoneId_0"] = wasmExports["ld"];

var _emscripten_bind_btJumpPad_setEnabled_1 = Module["_emscripten_bind_btJumpPad_setEnabled_1"] = wasmExports["md"];

var _emscripten_bind_btJumpPad_setUseExternalVelocity_1 = Module["_emscripten_bind_btJumpPad_setUseExternalVelocity_1"] = wasmExports["nd"];

var _emscripten_bind_btJumpPad___destroy___0 = Module["_emscripten_bind_btJumpPad___destroy___0"] = wasmExports["od"];

var _emscripten_bind_btBoostZone_btBoostZone_4 = Module["_emscripten_bind_btBoostZone_btBoostZone_4"] = wasmExports["pd"];

var _emscripten_bind_btBoostZone_setDirection_1 = Module["_emscripten_bind_btBoostZone_setDirection_1"] = wasmExports["qd"];

var _emscripten_bind_btBoostZone_setStrength_1 = Module["_emscripten_bind_btBoostZone_setStrength_1"] = wasmExports["rd"];

var _emscripten_bind_btBoostZone_setDirectionalBias_1 = Module["_emscripten_bind_btBoostZone_setDirectionalBias_1"] = wasmExports["sd"];

var _emscripten_bind_btBoostZone_getZoneId_0 = Module["_emscripten_bind_btBoostZone_getZoneId_0"] = wasmExports["td"];

var _emscripten_bind_btBoostZone_setEnabled_1 = Module["_emscripten_bind_btBoostZone_setEnabled_1"] = wasmExports["ud"];

var _emscripten_bind_btBoostZone___destroy___0 = Module["_emscripten_bind_btBoostZone___destroy___0"] = wasmExports["vd"];

var _emscripten_bind_btSensor_btSensor_3 = Module["_emscripten_bind_btSensor_btSensor_3"] = wasmExports["wd"];

var _emscripten_bind_btSensor_getZoneId_0 = Module["_emscripten_bind_btSensor_getZoneId_0"] = wasmExports["xd"];

var _emscripten_bind_btSensor_isOverlapping_0 = Module["_emscripten_bind_btSensor_isOverlapping_0"] = wasmExports["yd"];

var _emscripten_bind_btSensor_setEnabled_1 = Module["_emscripten_bind_btSensor_setEnabled_1"] = wasmExports["zd"];

var _emscripten_bind_btSensor___destroy___0 = Module["_emscripten_bind_btSensor___destroy___0"] = wasmExports["Ad"];

var _emscripten_bind_btDashToken_btDashToken_4 = Module["_emscripten_bind_btDashToken_btDashToken_4"] = wasmExports["Bd"];

var _emscripten_bind_btDashToken_getZoneId_0 = Module["_emscripten_bind_btDashToken_getZoneId_0"] = wasmExports["Cd"];

var _emscripten_bind_btDashToken_setEnabled_1 = Module["_emscripten_bind_btDashToken_setEnabled_1"] = wasmExports["Dd"];

var _emscripten_bind_btDashToken_setChargesGranted_1 = Module["_emscripten_bind_btDashToken_setChargesGranted_1"] = wasmExports["Ed"];

var _emscripten_bind_btDashToken_getChargesGranted_0 = Module["_emscripten_bind_btDashToken_getChargesGranted_0"] = wasmExports["Fd"];

var _emscripten_bind_btDashToken_setActive_1 = Module["_emscripten_bind_btDashToken_setActive_1"] = wasmExports["Gd"];

var _emscripten_bind_btDashToken_isActive_0 = Module["_emscripten_bind_btDashToken_isActive_0"] = wasmExports["Hd"];

var _emscripten_bind_btDashToken___destroy___0 = Module["_emscripten_bind_btDashToken___destroy___0"] = wasmExports["Id"];

var _emscripten_bind_btKinematicCharacterController_btKinematicCharacterController_3 = Module["_emscripten_bind_btKinematicCharacterController_btKinematicCharacterController_3"] = wasmExports["Jd"];

var _emscripten_bind_btKinematicCharacterController_btKinematicCharacterController_4 = Module["_emscripten_bind_btKinematicCharacterController_btKinematicCharacterController_4"] = wasmExports["Kd"];

var _emscripten_bind_btKinematicCharacterController_setInputState_4 = Module["_emscripten_bind_btKinematicCharacterController_setInputState_4"] = wasmExports["Ld"];

var _emscripten_bind_btKinematicCharacterController_setMoveSpeed_2 = Module["_emscripten_bind_btKinematicCharacterController_setMoveSpeed_2"] = wasmExports["Md"];

var _emscripten_bind_btKinematicCharacterController_isBoostEffective_0 = Module["_emscripten_bind_btKinematicCharacterController_isBoostEffective_0"] = wasmExports["Nd"];

var _emscripten_bind_btKinematicCharacterController_getBoostChargeRatio_0 = Module["_emscripten_bind_btKinematicCharacterController_getBoostChargeRatio_0"] = wasmExports["Od"];

var _emscripten_bind_btKinematicCharacterController_defineSurfaceMaterial_1 = Module["_emscripten_bind_btKinematicCharacterController_defineSurfaceMaterial_1"] = wasmExports["Pd"];

var _emscripten_bind_btKinematicCharacterController_setSurfaceMaterialBoost_5 = Module["_emscripten_bind_btKinematicCharacterController_setSurfaceMaterialBoost_5"] = wasmExports["Qd"];

var _emscripten_bind_btKinematicCharacterController_setSurfaceMaterialClimb_4 = Module["_emscripten_bind_btKinematicCharacterController_setSurfaceMaterialClimb_4"] = wasmExports["Rd"];

var _emscripten_bind_btKinematicCharacterController_setSurfaceMaterialExtVelGroundDamping_4 = Module["_emscripten_bind_btKinematicCharacterController_setSurfaceMaterialExtVelGroundDamping_4"] = wasmExports["Sd"];

var _emscripten_bind_btKinematicCharacterController_assignSurfaceMaterial_2 = Module["_emscripten_bind_btKinematicCharacterController_assignSurfaceMaterial_2"] = wasmExports["Td"];

var _emscripten_bind_btKinematicCharacterController_getFloorSurfaceMaterialId_0 = Module["_emscripten_bind_btKinematicCharacterController_getFloorSurfaceMaterialId_0"] = wasmExports["Ud"];

var _emscripten_bind_btKinematicCharacterController_setTopDownMode_1 = Module["_emscripten_bind_btKinematicCharacterController_setTopDownMode_1"] = wasmExports["Vd"];

var _emscripten_bind_btKinematicCharacterController_setMinJumpDelay_1 = Module["_emscripten_bind_btKinematicCharacterController_setMinJumpDelay_1"] = wasmExports["Wd"];

var _emscripten_bind_btKinematicCharacterController_setCoyoteTime_1 = Module["_emscripten_bind_btKinematicCharacterController_setCoyoteTime_1"] = wasmExports["Xd"];

var _emscripten_bind_btKinematicCharacterController_setBoostArmLeniency_1 = Module["_emscripten_bind_btKinematicCharacterController_setBoostArmLeniency_1"] = wasmExports["Yd"];

var _emscripten_bind_btKinematicCharacterController_setDashConfig_4 = Module["_emscripten_bind_btKinematicCharacterController_setDashConfig_4"] = wasmExports["Zd"];

var _emscripten_bind_btKinematicCharacterController_setDashDirectionMode_1 = Module["_emscripten_bind_btKinematicCharacterController_setDashDirectionMode_1"] = wasmExports["_d"];

var _emscripten_bind_btKinematicCharacterController_setDashVerticalUseJump_1 = Module["_emscripten_bind_btKinematicCharacterController_setDashVerticalUseJump_1"] = wasmExports["$d"];

var _emscripten_bind_btKinematicCharacterController_setDashCancelFallVelocity_1 = Module["_emscripten_bind_btKinematicCharacterController_setDashCancelFallVelocity_1"] = wasmExports["ae"];

var _emscripten_bind_btKinematicCharacterController_setDashCharges_1 = Module["_emscripten_bind_btKinematicCharacterController_setDashCharges_1"] = wasmExports["be"];

var _emscripten_bind_btKinematicCharacterController_getLastJumpTime_0 = Module["_emscripten_bind_btKinematicCharacterController_getLastJumpTime_0"] = wasmExports["ce"];

var _emscripten_bind_btKinematicCharacterController_getLastDashTime_0 = Module["_emscripten_bind_btKinematicCharacterController_getLastDashTime_0"] = wasmExports["de"];

var _emscripten_bind_btKinematicCharacterController_getDashCharges_0 = Module["_emscripten_bind_btKinematicCharacterController_getDashCharges_0"] = wasmExports["ee"];

var _emscripten_bind_btKinematicCharacterController_getLastMoveDir_0 = Module["_emscripten_bind_btKinematicCharacterController_getLastMoveDir_0"] = wasmExports["fe"];

var _emscripten_bind_btKinematicCharacterController_getLastDashDir_0 = Module["_emscripten_bind_btKinematicCharacterController_getLastDashDir_0"] = wasmExports["ge"];

var _emscripten_bind_btKinematicCharacterController_getWalkDirection_0 = Module["_emscripten_bind_btKinematicCharacterController_getWalkDirection_0"] = wasmExports["he"];

var _emscripten_bind_btKinematicCharacterController_warp_1 = Module["_emscripten_bind_btKinematicCharacterController_warp_1"] = wasmExports["ie"];

var _emscripten_bind_btKinematicCharacterController_getPosition_0 = Module["_emscripten_bind_btKinematicCharacterController_getPosition_0"] = wasmExports["je"];

var _emscripten_bind_btKinematicCharacterController_setFallSpeed_1 = Module["_emscripten_bind_btKinematicCharacterController_setFallSpeed_1"] = wasmExports["ke"];

var _emscripten_bind_btKinematicCharacterController_setJumpSpeed_1 = Module["_emscripten_bind_btKinematicCharacterController_setJumpSpeed_1"] = wasmExports["le"];

var _emscripten_bind_btKinematicCharacterController_setGravity_1 = Module["_emscripten_bind_btKinematicCharacterController_setGravity_1"] = wasmExports["me"];

var _emscripten_bind_btKinematicCharacterController_setMaxSlope_1 = Module["_emscripten_bind_btKinematicCharacterController_setMaxSlope_1"] = wasmExports["ne"];

var _emscripten_bind_btKinematicCharacterController_setMaxPenetrationDepth_1 = Module["_emscripten_bind_btKinematicCharacterController_setMaxPenetrationDepth_1"] = wasmExports["oe"];

var _emscripten_bind_btKinematicCharacterController_setStepHeight_1 = Module["_emscripten_bind_btKinematicCharacterController_setStepHeight_1"] = wasmExports["pe"];

var _emscripten_bind_btKinematicCharacterController_setExternalVelocity_1 = Module["_emscripten_bind_btKinematicCharacterController_setExternalVelocity_1"] = wasmExports["qe"];

var _emscripten_bind_btKinematicCharacterController_setExternalVelocityAirDampingFactor_1 = Module["_emscripten_bind_btKinematicCharacterController_setExternalVelocityAirDampingFactor_1"] = wasmExports["re"];

var _emscripten_bind_btKinematicCharacterController_setExternalVelocityAirIdleDampingFactor_1 = Module["_emscripten_bind_btKinematicCharacterController_setExternalVelocityAirIdleDampingFactor_1"] = wasmExports["se"];

var _emscripten_bind_btKinematicCharacterController_setExternalVelocityGroundDampingFactor_1 = Module["_emscripten_bind_btKinematicCharacterController_setExternalVelocityGroundDampingFactor_1"] = wasmExports["te"];

var _emscripten_bind_btKinematicCharacterController_setSlopeSlide_2 = Module["_emscripten_bind_btKinematicCharacterController_setSlopeSlide_2"] = wasmExports["ue"];

var _emscripten_bind_btKinematicCharacterController_setOnGround_1 = Module["_emscripten_bind_btKinematicCharacterController_setOnGround_1"] = wasmExports["ve"];

var _emscripten_bind_btKinematicCharacterController_setVerticalVelocity_1 = Module["_emscripten_bind_btKinematicCharacterController_setVerticalVelocity_1"] = wasmExports["we"];

var _emscripten_bind_btKinematicCharacterController_setGravityShapeRiseMultiplier_1 = Module["_emscripten_bind_btKinematicCharacterController_setGravityShapeRiseMultiplier_1"] = wasmExports["xe"];

var _emscripten_bind_btKinematicCharacterController_setGravityShapeApexMultiplier_1 = Module["_emscripten_bind_btKinematicCharacterController_setGravityShapeApexMultiplier_1"] = wasmExports["ye"];

var _emscripten_bind_btKinematicCharacterController_setGravityShapeFallMultiplier_1 = Module["_emscripten_bind_btKinematicCharacterController_setGravityShapeFallMultiplier_1"] = wasmExports["ze"];

var _emscripten_bind_btKinematicCharacterController_setGravityShapeApexThreshold_1 = Module["_emscripten_bind_btKinematicCharacterController_setGravityShapeApexThreshold_1"] = wasmExports["Ae"];

var _emscripten_bind_btKinematicCharacterController_setGravityShapeKneeWidth_1 = Module["_emscripten_bind_btKinematicCharacterController_setGravityShapeKneeWidth_1"] = wasmExports["Be"];

var _emscripten_bind_btKinematicCharacterController_setGravityShapeOnlyJumps_1 = Module["_emscripten_bind_btKinematicCharacterController_setGravityShapeOnlyJumps_1"] = wasmExports["Ce"];

var _emscripten_bind_btKinematicCharacterController_onGround_0 = Module["_emscripten_bind_btKinematicCharacterController_onGround_0"] = wasmExports["De"];

var _emscripten_bind_btKinematicCharacterController_getVerticalVelocity_0 = Module["_emscripten_bind_btKinematicCharacterController_getVerticalVelocity_0"] = wasmExports["Ee"];

var _emscripten_bind_btKinematicCharacterController_getJumpAxis_0 = Module["_emscripten_bind_btKinematicCharacterController_getJumpAxis_0"] = wasmExports["Fe"];

var _emscripten_bind_btKinematicCharacterController_getExternalVelocity_0 = Module["_emscripten_bind_btKinematicCharacterController_getExternalVelocity_0"] = wasmExports["Ge"];

var _emscripten_bind_btKinematicCharacterController_isJumping_0 = Module["_emscripten_bind_btKinematicCharacterController_isJumping_0"] = wasmExports["He"];

var _emscripten_bind_btKinematicCharacterController_getFloorUserIndex_0 = Module["_emscripten_bind_btKinematicCharacterController_getFloorUserIndex_0"] = wasmExports["Ie"];

var _emscripten_bind_btKinematicCharacterController_addJumpPad_1 = Module["_emscripten_bind_btKinematicCharacterController_addJumpPad_1"] = wasmExports["Je"];

var _emscripten_bind_btKinematicCharacterController_removeJumpPad_1 = Module["_emscripten_bind_btKinematicCharacterController_removeJumpPad_1"] = wasmExports["Ke"];

var _emscripten_bind_btKinematicCharacterController_addBoostZone_1 = Module["_emscripten_bind_btKinematicCharacterController_addBoostZone_1"] = wasmExports["Le"];

var _emscripten_bind_btKinematicCharacterController_removeBoostZone_1 = Module["_emscripten_bind_btKinematicCharacterController_removeBoostZone_1"] = wasmExports["Me"];

var _emscripten_bind_btKinematicCharacterController_addSensor_1 = Module["_emscripten_bind_btKinematicCharacterController_addSensor_1"] = wasmExports["Ne"];

var _emscripten_bind_btKinematicCharacterController_removeSensor_1 = Module["_emscripten_bind_btKinematicCharacterController_removeSensor_1"] = wasmExports["Oe"];

var _emscripten_bind_btKinematicCharacterController_addDashToken_1 = Module["_emscripten_bind_btKinematicCharacterController_addDashToken_1"] = wasmExports["Pe"];

var _emscripten_bind_btKinematicCharacterController_removeDashToken_1 = Module["_emscripten_bind_btKinematicCharacterController_removeDashToken_1"] = wasmExports["Qe"];

var _emscripten_bind_btKinematicCharacterController_captureInitialDashState_0 = Module["_emscripten_bind_btKinematicCharacterController_captureInitialDashState_0"] = wasmExports["Re"];

var _emscripten_bind_btKinematicCharacterController_saveDashCheckpointState_0 = Module["_emscripten_bind_btKinematicCharacterController_saveDashCheckpointState_0"] = wasmExports["Se"];

var _emscripten_bind_btKinematicCharacterController_restoreDashCheckpointState_0 = Module["_emscripten_bind_btKinematicCharacterController_restoreDashCheckpointState_0"] = wasmExports["Te"];

var _emscripten_bind_btKinematicCharacterController_resetDashStateForNewRun_0 = Module["_emscripten_bind_btKinematicCharacterController_resetDashStateForNewRun_0"] = wasmExports["Ue"];

var _emscripten_bind_btKinematicCharacterController_getNumPendingEvents_0 = Module["_emscripten_bind_btKinematicCharacterController_getNumPendingEvents_0"] = wasmExports["Ve"];

var _emscripten_bind_btKinematicCharacterController_getPendingEventId_1 = Module["_emscripten_bind_btKinematicCharacterController_getPendingEventId_1"] = wasmExports["We"];

var _emscripten_bind_btKinematicCharacterController_getPendingEventType_1 = Module["_emscripten_bind_btKinematicCharacterController_getPendingEventType_1"] = wasmExports["Xe"];

var _emscripten_bind_btKinematicCharacterController_clearPendingEvents_0 = Module["_emscripten_bind_btKinematicCharacterController_clearPendingEvents_0"] = wasmExports["Ye"];

var _emscripten_bind_btKinematicCharacterController_cameraRayTest_7 = Module["_emscripten_bind_btKinematicCharacterController_cameraRayTest_7"] = wasmExports["Ze"];

var _emscripten_bind_btKinematicCharacterController_cameraSphereSweep_9 = Module["_emscripten_bind_btKinematicCharacterController_cameraSphereSweep_9"] = wasmExports["_e"];

var _emscripten_bind_btKinematicCharacterController_castShadowProbeGrid_8 = Module["_emscripten_bind_btKinematicCharacterController_castShadowProbeGrid_8"] = wasmExports["$e"];

var _emscripten_bind_btKinematicCharacterController_getCameraRayHitNormalX_0 = Module["_emscripten_bind_btKinematicCharacterController_getCameraRayHitNormalX_0"] = wasmExports["af"];

var _emscripten_bind_btKinematicCharacterController_getCameraRayHitNormalY_0 = Module["_emscripten_bind_btKinematicCharacterController_getCameraRayHitNormalY_0"] = wasmExports["bf"];

var _emscripten_bind_btKinematicCharacterController_getCameraRayHitNormalZ_0 = Module["_emscripten_bind_btKinematicCharacterController_getCameraRayHitNormalZ_0"] = wasmExports["cf"];

var _emscripten_bind_btKinematicCharacterController_getCameraRayHitNonPermeable_0 = Module["_emscripten_bind_btKinematicCharacterController_getCameraRayHitNonPermeable_0"] = wasmExports["df"];

var _emscripten_bind_btKinematicCharacterController_packState_1 = Module["_emscripten_bind_btKinematicCharacterController_packState_1"] = wasmExports["ef"];

var _emscripten_bind_btKinematicCharacterController_resetForNewRun_0 = Module["_emscripten_bind_btKinematicCharacterController_resetForNewRun_0"] = wasmExports["ff"];

var _emscripten_bind_btKinematicCharacterController_resetCollisionCache_3 = Module["_emscripten_bind_btKinematicCharacterController_resetCollisionCache_3"] = wasmExports["gf"];

var _emscripten_bind_btKinematicCharacterController___destroy___0 = Module["_emscripten_bind_btKinematicCharacterController___destroy___0"] = wasmExports["hf"];

var _emscripten_bind_btPairCachingGhostObject_btPairCachingGhostObject_0 = Module["_emscripten_bind_btPairCachingGhostObject_btPairCachingGhostObject_0"] = wasmExports["jf"];

var _emscripten_bind_btPairCachingGhostObject_getCollisionShape_0 = Module["_emscripten_bind_btPairCachingGhostObject_getCollisionShape_0"] = wasmExports["kf"];

var _emscripten_bind_btPairCachingGhostObject_setActivationState_1 = Module["_emscripten_bind_btPairCachingGhostObject_setActivationState_1"] = wasmExports["lf"];

var _emscripten_bind_btPairCachingGhostObject_getWorldTransform_0 = Module["_emscripten_bind_btPairCachingGhostObject_getWorldTransform_0"] = wasmExports["mf"];

var _emscripten_bind_btPairCachingGhostObject_setCollisionFlags_1 = Module["_emscripten_bind_btPairCachingGhostObject_setCollisionFlags_1"] = wasmExports["nf"];

var _emscripten_bind_btPairCachingGhostObject_setWorldTransform_1 = Module["_emscripten_bind_btPairCachingGhostObject_setWorldTransform_1"] = wasmExports["of"];

var _emscripten_bind_btPairCachingGhostObject_setCollisionShape_1 = Module["_emscripten_bind_btPairCachingGhostObject_setCollisionShape_1"] = wasmExports["pf"];

var _emscripten_bind_btPairCachingGhostObject_getUserIndex_0 = Module["_emscripten_bind_btPairCachingGhostObject_getUserIndex_0"] = wasmExports["qf"];

var _emscripten_bind_btPairCachingGhostObject_setUserIndex_1 = Module["_emscripten_bind_btPairCachingGhostObject_setUserIndex_1"] = wasmExports["rf"];

var _emscripten_bind_btPairCachingGhostObject_setUserIndex2_1 = Module["_emscripten_bind_btPairCachingGhostObject_setUserIndex2_1"] = wasmExports["sf"];

var _emscripten_bind_btPairCachingGhostObject_getNumOverlappingObjects_0 = Module["_emscripten_bind_btPairCachingGhostObject_getNumOverlappingObjects_0"] = wasmExports["tf"];

var _emscripten_bind_btPairCachingGhostObject___destroy___0 = Module["_emscripten_bind_btPairCachingGhostObject___destroy___0"] = wasmExports["uf"];

var _emscripten_bind_btGhostPairCallback_btGhostPairCallback_0 = Module["_emscripten_bind_btGhostPairCallback_btGhostPairCallback_0"] = wasmExports["vf"];

var _emscripten_bind_btGhostPairCallback___destroy___0 = Module["_emscripten_bind_btGhostPairCallback___destroy___0"] = wasmExports["wf"];

var _emscripten_enum_PHY_ScalarType_PHY_FLOAT = Module["_emscripten_enum_PHY_ScalarType_PHY_FLOAT"] = wasmExports["xf"];

var _emscripten_enum_PHY_ScalarType_PHY_DOUBLE = Module["_emscripten_enum_PHY_ScalarType_PHY_DOUBLE"] = wasmExports["yf"];

var _emscripten_enum_PHY_ScalarType_PHY_INTEGER = Module["_emscripten_enum_PHY_ScalarType_PHY_INTEGER"] = wasmExports["zf"];

var _emscripten_enum_PHY_ScalarType_PHY_SHORT = Module["_emscripten_enum_PHY_ScalarType_PHY_SHORT"] = wasmExports["Af"];

var _emscripten_enum_PHY_ScalarType_PHY_FIXEDPOINT88 = Module["_emscripten_enum_PHY_ScalarType_PHY_FIXEDPOINT88"] = wasmExports["Bf"];

var _emscripten_enum_PHY_ScalarType_PHY_UCHAR = Module["_emscripten_enum_PHY_ScalarType_PHY_UCHAR"] = wasmExports["Cf"];

var __emscripten_timeout = wasmExports["Df"];

// include: postamble.js
// === Auto-generated postamble setup entry stuff ===
Module["addFunction"] = addFunction;

Module["UTF8ToString"] = UTF8ToString;

function run() {
  if (runDependencies > 0) {
    dependenciesFulfilled = run;
    return;
  }
  preRun();
  // a preRun added a dependency, run will be called later
  if (runDependencies > 0) {
    dependenciesFulfilled = run;
    return;
  }
  function doRun() {
    // run may have just been called through dependencies being fulfilled just in this very frame,
    // or while the async setStatus time below was happening
    Module["calledRun"] = true;
    if (ABORT) return;
    initRuntime();
    readyPromiseResolve(Module);
    Module["onRuntimeInitialized"]?.();
    postRun();
  }
  if (Module["setStatus"]) {
    Module["setStatus"]("Running...");
    setTimeout(() => {
      setTimeout(() => Module["setStatus"](""), 1);
      doRun();
    }, 1);
  } else {
    doRun();
  }
}

if (Module["preInit"]) {
  if (typeof Module["preInit"] == "function") Module["preInit"] = [ Module["preInit"] ];
  while (Module["preInit"].length > 0) {
    Module["preInit"].pop()();
  }
}

run();

// end include: postamble.js
// include: glue.js
// Bindings utilities
/** @suppress {duplicate} (TODO: avoid emitting this multiple times, it is redundant) */ function WrapperObject() {}

WrapperObject.prototype = Object.create(WrapperObject.prototype);

WrapperObject.prototype.constructor = WrapperObject;

WrapperObject.prototype.__class__ = WrapperObject;

WrapperObject.__cache__ = {};

Module["WrapperObject"] = WrapperObject;

/** @suppress {duplicate} (TODO: avoid emitting this multiple times, it is redundant)
    @param {*=} __class__ */ function getCache(__class__) {
  return (__class__ || WrapperObject).__cache__;
}

Module["getCache"] = getCache;

/** @suppress {duplicate} (TODO: avoid emitting this multiple times, it is redundant)
    @param {*=} __class__ */ function wrapPointer(ptr, __class__) {
  var cache = getCache(__class__);
  var ret = cache[ptr];
  if (ret) return ret;
  ret = Object.create((__class__ || WrapperObject).prototype);
  ret.ptr = ptr;
  return cache[ptr] = ret;
}

Module["wrapPointer"] = wrapPointer;

/** @suppress {duplicate} (TODO: avoid emitting this multiple times, it is redundant) */ function castObject(obj, __class__) {
  return wrapPointer(obj.ptr, __class__);
}

Module["castObject"] = castObject;

Module["NULL"] = wrapPointer(0);

/** @suppress {duplicate} (TODO: avoid emitting this multiple times, it is redundant) */ function destroy(obj) {
  if (!obj["__destroy__"]) throw "Error: Cannot destroy object. (Did you create it yourself?)";
  obj["__destroy__"]();
  // Remove from cache, so the object can be GC'd and refs added onto it released
  delete getCache(obj.__class__)[obj.ptr];
}

Module["destroy"] = destroy;

/** @suppress {duplicate} (TODO: avoid emitting this multiple times, it is redundant) */ function compare(obj1, obj2) {
  return obj1.ptr === obj2.ptr;
}

Module["compare"] = compare;

/** @suppress {duplicate} (TODO: avoid emitting this multiple times, it is redundant) */ function getPointer(obj) {
  return obj.ptr;
}

Module["getPointer"] = getPointer;

/** @suppress {duplicate} (TODO: avoid emitting this multiple times, it is redundant) */ function getClass(obj) {
  return obj.__class__;
}

Module["getClass"] = getClass;

// Converts big (string or array) values into a C-style storage, in temporary space
/** @suppress {duplicate} (TODO: avoid emitting this multiple times, it is redundant) */ var ensureCache = {
  buffer: 0,
  // the main buffer of temporary storage
  size: 0,
  // the size of buffer
  pos: 0,
  // the next free offset in buffer
  temps: [],
  // extra allocations
  needed: 0,
  // the total size we need next time
  prepare() {
    if (ensureCache.needed) {
      // clear the temps
      for (var i = 0; i < ensureCache.temps.length; i++) {
        Module["_webidl_free"](ensureCache.temps[i]);
      }
      ensureCache.temps.length = 0;
      // prepare to allocate a bigger buffer
      Module["_webidl_free"](ensureCache.buffer);
      ensureCache.buffer = 0;
      ensureCache.size += ensureCache.needed;
      // clean up
      ensureCache.needed = 0;
    }
    if (!ensureCache.buffer) {
      // happens first time, or when we need to grow
      ensureCache.size += 128;
      // heuristic, avoid many small grow events
      ensureCache.buffer = Module["_webidl_malloc"](ensureCache.size);
      assert(ensureCache.buffer);
    }
    ensureCache.pos = 0;
  },
  alloc(array, view) {
    assert(ensureCache.buffer);
    var bytes = view.BYTES_PER_ELEMENT;
    var len = array.length * bytes;
    len = alignMemory(len, 8);
    // keep things aligned to 8 byte boundaries
    var ret;
    if (ensureCache.pos + len >= ensureCache.size) {
      // we failed to allocate in the buffer, ensureCache time around :(
      assert(len > 0);
      // null terminator, at least
      ensureCache.needed += len;
      ret = Module["_webidl_malloc"](len);
      ensureCache.temps.push(ret);
    } else {
      // we can allocate in the buffer
      ret = ensureCache.buffer + ensureCache.pos;
      ensureCache.pos += len;
    }
    return ret;
  },
  copy(array, view, offset) {
    offset /= view.BYTES_PER_ELEMENT;
    for (var i = 0; i < array.length; i++) {
      view[offset + i] = array[i];
    }
  }
};

/** @suppress {duplicate} (TODO: avoid emitting this multiple times, it is redundant) */ function ensureFloat32(value) {
  if (typeof value === "object") {
    var offset = ensureCache.alloc(value, HEAPF32);
    ensureCache.copy(value, HEAPF32, offset);
    return offset;
  }
  return value;
}

// Interface: btCollisionObject
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btCollisionObject() {
  throw "cannot construct a btCollisionObject, no constructor in IDL";
}

btCollisionObject.prototype = Object.create(WrapperObject.prototype);

btCollisionObject.prototype.constructor = btCollisionObject;

btCollisionObject.prototype.__class__ = btCollisionObject;

btCollisionObject.__cache__ = {};

Module["btCollisionObject"] = btCollisionObject;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionObject.prototype["getCollisionShape"] = btCollisionObject.prototype.getCollisionShape = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btCollisionObject_getCollisionShape_0(self), btCollisionShape);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionObject.prototype["setActivationState"] = btCollisionObject.prototype.setActivationState = function(newState) {
  var self = this.ptr;
  if (newState && typeof newState === "object") newState = newState.ptr;
  _emscripten_bind_btCollisionObject_setActivationState_1(self, newState);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionObject.prototype["getWorldTransform"] = btCollisionObject.prototype.getWorldTransform = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btCollisionObject_getWorldTransform_0(self), btTransform);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionObject.prototype["setCollisionFlags"] = btCollisionObject.prototype.setCollisionFlags = function(flags) {
  var self = this.ptr;
  if (flags && typeof flags === "object") flags = flags.ptr;
  _emscripten_bind_btCollisionObject_setCollisionFlags_1(self, flags);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionObject.prototype["setWorldTransform"] = btCollisionObject.prototype.setWorldTransform = function(worldTrans) {
  var self = this.ptr;
  if (worldTrans && typeof worldTrans === "object") worldTrans = worldTrans.ptr;
  _emscripten_bind_btCollisionObject_setWorldTransform_1(self, worldTrans);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionObject.prototype["setCollisionShape"] = btCollisionObject.prototype.setCollisionShape = function(collisionShape) {
  var self = this.ptr;
  if (collisionShape && typeof collisionShape === "object") collisionShape = collisionShape.ptr;
  _emscripten_bind_btCollisionObject_setCollisionShape_1(self, collisionShape);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionObject.prototype["getUserIndex"] = btCollisionObject.prototype.getUserIndex = function() {
  var self = this.ptr;
  return _emscripten_bind_btCollisionObject_getUserIndex_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionObject.prototype["setUserIndex"] = btCollisionObject.prototype.setUserIndex = function(index) {
  var self = this.ptr;
  if (index && typeof index === "object") index = index.ptr;
  _emscripten_bind_btCollisionObject_setUserIndex_1(self, index);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionObject.prototype["setUserIndex2"] = btCollisionObject.prototype.setUserIndex2 = function(index) {
  var self = this.ptr;
  if (index && typeof index === "object") index = index.ptr;
  _emscripten_bind_btCollisionObject_setUserIndex2_1(self, index);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionObject.prototype["__destroy__"] = btCollisionObject.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btCollisionObject___destroy___0(self);
};

// Interface: btCollisionShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btCollisionShape() {
  throw "cannot construct a btCollisionShape, no constructor in IDL";
}

btCollisionShape.prototype = Object.create(WrapperObject.prototype);

btCollisionShape.prototype.constructor = btCollisionShape;

btCollisionShape.prototype.__class__ = btCollisionShape;

btCollisionShape.__cache__ = {};

Module["btCollisionShape"] = btCollisionShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionShape.prototype["setLocalScaling"] = btCollisionShape.prototype.setLocalScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btCollisionShape_setLocalScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionShape.prototype["__destroy__"] = btCollisionShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btCollisionShape___destroy___0(self);
};

// Interface: btCollisionWorld
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btCollisionWorld() {
  throw "cannot construct a btCollisionWorld, no constructor in IDL";
}

btCollisionWorld.prototype = Object.create(WrapperObject.prototype);

btCollisionWorld.prototype.constructor = btCollisionWorld;

btCollisionWorld.prototype.__class__ = btCollisionWorld;

btCollisionWorld.__cache__ = {};

Module["btCollisionWorld"] = btCollisionWorld;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionWorld.prototype["getDispatcher"] = btCollisionWorld.prototype.getDispatcher = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btCollisionWorld_getDispatcher_0(self), btDispatcher);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionWorld.prototype["getDispatchInfo"] = btCollisionWorld.prototype.getDispatchInfo = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btCollisionWorld_getDispatchInfo_0(self), btDispatcherInfo);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionWorld.prototype["addCollisionObject"] = btCollisionWorld.prototype.addCollisionObject = function(collisionObject, collisionFilterGroup, collisionFilterMask) {
  var self = this.ptr;
  if (collisionObject && typeof collisionObject === "object") collisionObject = collisionObject.ptr;
  if (collisionFilterGroup && typeof collisionFilterGroup === "object") collisionFilterGroup = collisionFilterGroup.ptr;
  if (collisionFilterMask && typeof collisionFilterMask === "object") collisionFilterMask = collisionFilterMask.ptr;
  if (collisionFilterGroup === undefined) {
    _emscripten_bind_btCollisionWorld_addCollisionObject_1(self, collisionObject);
    return;
  }
  if (collisionFilterMask === undefined) {
    _emscripten_bind_btCollisionWorld_addCollisionObject_2(self, collisionObject, collisionFilterGroup);
    return;
  }
  _emscripten_bind_btCollisionWorld_addCollisionObject_3(self, collisionObject, collisionFilterGroup, collisionFilterMask);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionWorld.prototype["removeCollisionObject"] = btCollisionWorld.prototype.removeCollisionObject = function(collisionObject) {
  var self = this.ptr;
  if (collisionObject && typeof collisionObject === "object") collisionObject = collisionObject.ptr;
  _emscripten_bind_btCollisionWorld_removeCollisionObject_1(self, collisionObject);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionWorld.prototype["getBroadphase"] = btCollisionWorld.prototype.getBroadphase = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btCollisionWorld_getBroadphase_0(self), btBroadphaseInterface);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionWorld.prototype["contactPairTestBinary"] = btCollisionWorld.prototype.contactPairTestBinary = function(colObjA, colObjB, minPenetrationDepth) {
  var self = this.ptr;
  if (colObjA && typeof colObjA === "object") colObjA = colObjA.ptr;
  if (colObjB && typeof colObjB === "object") colObjB = colObjB.ptr;
  if (minPenetrationDepth && typeof minPenetrationDepth === "object") minPenetrationDepth = minPenetrationDepth.ptr;
  return !!(_emscripten_bind_btCollisionWorld_contactPairTestBinary_3(self, colObjA, colObjB, minPenetrationDepth));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionWorld.prototype["__destroy__"] = btCollisionWorld.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btCollisionWorld___destroy___0(self);
};

// Interface: btMotionState
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btMotionState() {
  throw "cannot construct a btMotionState, no constructor in IDL";
}

btMotionState.prototype = Object.create(WrapperObject.prototype);

btMotionState.prototype.constructor = btMotionState;

btMotionState.prototype.__class__ = btMotionState;

btMotionState.__cache__ = {};

Module["btMotionState"] = btMotionState;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btMotionState.prototype["getWorldTransform"] = btMotionState.prototype.getWorldTransform = function(worldTrans) {
  var self = this.ptr;
  if (worldTrans && typeof worldTrans === "object") worldTrans = worldTrans.ptr;
  _emscripten_bind_btMotionState_getWorldTransform_1(self, worldTrans);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btMotionState.prototype["setWorldTransform"] = btMotionState.prototype.setWorldTransform = function(worldTrans) {
  var self = this.ptr;
  if (worldTrans && typeof worldTrans === "object") worldTrans = worldTrans.ptr;
  _emscripten_bind_btMotionState_setWorldTransform_1(self, worldTrans);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btMotionState.prototype["__destroy__"] = btMotionState.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btMotionState___destroy___0(self);
};

// Interface: btStridingMeshInterface
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btStridingMeshInterface() {
  throw "cannot construct a btStridingMeshInterface, no constructor in IDL";
}

btStridingMeshInterface.prototype = Object.create(WrapperObject.prototype);

btStridingMeshInterface.prototype.constructor = btStridingMeshInterface;

btStridingMeshInterface.prototype.__class__ = btStridingMeshInterface;

btStridingMeshInterface.__cache__ = {};

Module["btStridingMeshInterface"] = btStridingMeshInterface;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btStridingMeshInterface.prototype["__destroy__"] = btStridingMeshInterface.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btStridingMeshInterface___destroy___0(self);
};

// Interface: btConcaveShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btConcaveShape() {
  throw "cannot construct a btConcaveShape, no constructor in IDL";
}

btConcaveShape.prototype = Object.create(btCollisionShape.prototype);

btConcaveShape.prototype.constructor = btConcaveShape;

btConcaveShape.prototype.__class__ = btConcaveShape;

btConcaveShape.__cache__ = {};

Module["btConcaveShape"] = btConcaveShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btConcaveShape.prototype["setLocalScaling"] = btConcaveShape.prototype.setLocalScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btConcaveShape_setLocalScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btConcaveShape.prototype["__destroy__"] = btConcaveShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btConcaveShape___destroy___0(self);
};

// Interface: btConstraintSolver
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btConstraintSolver() {
  throw "cannot construct a btConstraintSolver, no constructor in IDL";
}

btConstraintSolver.prototype = Object.create(WrapperObject.prototype);

btConstraintSolver.prototype.constructor = btConstraintSolver;

btConstraintSolver.prototype.__class__ = btConstraintSolver;

btConstraintSolver.__cache__ = {};

Module["btConstraintSolver"] = btConstraintSolver;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btConstraintSolver.prototype["__destroy__"] = btConstraintSolver.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btConstraintSolver___destroy___0(self);
};

// Interface: btDynamicsWorld
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btDynamicsWorld() {
  throw "cannot construct a btDynamicsWorld, no constructor in IDL";
}

btDynamicsWorld.prototype = Object.create(btCollisionWorld.prototype);

btDynamicsWorld.prototype.constructor = btDynamicsWorld;

btDynamicsWorld.prototype.__class__ = btDynamicsWorld;

btDynamicsWorld.__cache__ = {};

Module["btDynamicsWorld"] = btDynamicsWorld;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDynamicsWorld.prototype["addAction"] = btDynamicsWorld.prototype.addAction = function(action) {
  var self = this.ptr;
  if (action && typeof action === "object") action = action.ptr;
  _emscripten_bind_btDynamicsWorld_addAction_1(self, action);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDynamicsWorld.prototype["getDispatcher"] = btDynamicsWorld.prototype.getDispatcher = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btDynamicsWorld_getDispatcher_0(self), btDispatcher);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDynamicsWorld.prototype["getDispatchInfo"] = btDynamicsWorld.prototype.getDispatchInfo = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btDynamicsWorld_getDispatchInfo_0(self), btDispatcherInfo);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDynamicsWorld.prototype["addCollisionObject"] = btDynamicsWorld.prototype.addCollisionObject = function(collisionObject, collisionFilterGroup, collisionFilterMask) {
  var self = this.ptr;
  if (collisionObject && typeof collisionObject === "object") collisionObject = collisionObject.ptr;
  if (collisionFilterGroup && typeof collisionFilterGroup === "object") collisionFilterGroup = collisionFilterGroup.ptr;
  if (collisionFilterMask && typeof collisionFilterMask === "object") collisionFilterMask = collisionFilterMask.ptr;
  if (collisionFilterGroup === undefined) {
    _emscripten_bind_btDynamicsWorld_addCollisionObject_1(self, collisionObject);
    return;
  }
  if (collisionFilterMask === undefined) {
    _emscripten_bind_btDynamicsWorld_addCollisionObject_2(self, collisionObject, collisionFilterGroup);
    return;
  }
  _emscripten_bind_btDynamicsWorld_addCollisionObject_3(self, collisionObject, collisionFilterGroup, collisionFilterMask);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDynamicsWorld.prototype["removeCollisionObject"] = btDynamicsWorld.prototype.removeCollisionObject = function(collisionObject) {
  var self = this.ptr;
  if (collisionObject && typeof collisionObject === "object") collisionObject = collisionObject.ptr;
  _emscripten_bind_btDynamicsWorld_removeCollisionObject_1(self, collisionObject);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDynamicsWorld.prototype["getBroadphase"] = btDynamicsWorld.prototype.getBroadphase = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btDynamicsWorld_getBroadphase_0(self), btBroadphaseInterface);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDynamicsWorld.prototype["contactPairTestBinary"] = btDynamicsWorld.prototype.contactPairTestBinary = function(colObjA, colObjB, minPenetrationDepth) {
  var self = this.ptr;
  if (colObjA && typeof colObjA === "object") colObjA = colObjA.ptr;
  if (colObjB && typeof colObjB === "object") colObjB = colObjB.ptr;
  if (minPenetrationDepth && typeof minPenetrationDepth === "object") minPenetrationDepth = minPenetrationDepth.ptr;
  return !!(_emscripten_bind_btDynamicsWorld_contactPairTestBinary_3(self, colObjA, colObjB, minPenetrationDepth));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDynamicsWorld.prototype["__destroy__"] = btDynamicsWorld.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btDynamicsWorld___destroy___0(self);
};

// Interface: btActionInterface
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btActionInterface() {
  throw "cannot construct a btActionInterface, no constructor in IDL";
}

btActionInterface.prototype = Object.create(WrapperObject.prototype);

btActionInterface.prototype.constructor = btActionInterface;

btActionInterface.prototype.__class__ = btActionInterface;

btActionInterface.__cache__ = {};

Module["btActionInterface"] = btActionInterface;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btActionInterface.prototype["__destroy__"] = btActionInterface.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btActionInterface___destroy___0(self);
};

// Interface: btGhostObject
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btGhostObject() {
  this.ptr = _emscripten_bind_btGhostObject_btGhostObject_0();
  getCache(btGhostObject)[this.ptr] = this;
}

btGhostObject.prototype = Object.create(btCollisionObject.prototype);

btGhostObject.prototype.constructor = btGhostObject;

btGhostObject.prototype.__class__ = btGhostObject;

btGhostObject.__cache__ = {};

Module["btGhostObject"] = btGhostObject;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btGhostObject.prototype["getNumOverlappingObjects"] = btGhostObject.prototype.getNumOverlappingObjects = function() {
  var self = this.ptr;
  return _emscripten_bind_btGhostObject_getNumOverlappingObjects_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btGhostObject.prototype["getCollisionShape"] = btGhostObject.prototype.getCollisionShape = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btGhostObject_getCollisionShape_0(self), btCollisionShape);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btGhostObject.prototype["setActivationState"] = btGhostObject.prototype.setActivationState = function(newState) {
  var self = this.ptr;
  if (newState && typeof newState === "object") newState = newState.ptr;
  _emscripten_bind_btGhostObject_setActivationState_1(self, newState);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btGhostObject.prototype["getWorldTransform"] = btGhostObject.prototype.getWorldTransform = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btGhostObject_getWorldTransform_0(self), btTransform);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btGhostObject.prototype["setCollisionFlags"] = btGhostObject.prototype.setCollisionFlags = function(flags) {
  var self = this.ptr;
  if (flags && typeof flags === "object") flags = flags.ptr;
  _emscripten_bind_btGhostObject_setCollisionFlags_1(self, flags);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btGhostObject.prototype["setWorldTransform"] = btGhostObject.prototype.setWorldTransform = function(worldTrans) {
  var self = this.ptr;
  if (worldTrans && typeof worldTrans === "object") worldTrans = worldTrans.ptr;
  _emscripten_bind_btGhostObject_setWorldTransform_1(self, worldTrans);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btGhostObject.prototype["setCollisionShape"] = btGhostObject.prototype.setCollisionShape = function(collisionShape) {
  var self = this.ptr;
  if (collisionShape && typeof collisionShape === "object") collisionShape = collisionShape.ptr;
  _emscripten_bind_btGhostObject_setCollisionShape_1(self, collisionShape);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btGhostObject.prototype["getUserIndex"] = btGhostObject.prototype.getUserIndex = function() {
  var self = this.ptr;
  return _emscripten_bind_btGhostObject_getUserIndex_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btGhostObject.prototype["setUserIndex"] = btGhostObject.prototype.setUserIndex = function(index) {
  var self = this.ptr;
  if (index && typeof index === "object") index = index.ptr;
  _emscripten_bind_btGhostObject_setUserIndex_1(self, index);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btGhostObject.prototype["setUserIndex2"] = btGhostObject.prototype.setUserIndex2 = function(index) {
  var self = this.ptr;
  if (index && typeof index === "object") index = index.ptr;
  _emscripten_bind_btGhostObject_setUserIndex2_1(self, index);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btGhostObject.prototype["__destroy__"] = btGhostObject.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btGhostObject___destroy___0(self);
};

// Interface: VoidPtr
/** @suppress {undefinedVars, duplicate} @this{Object} */ function VoidPtr() {
  throw "cannot construct a VoidPtr, no constructor in IDL";
}

VoidPtr.prototype = Object.create(WrapperObject.prototype);

VoidPtr.prototype.constructor = VoidPtr;

VoidPtr.prototype.__class__ = VoidPtr;

VoidPtr.__cache__ = {};

Module["VoidPtr"] = VoidPtr;

/** @suppress {undefinedVars, duplicate} @this{Object} */ VoidPtr.prototype["__destroy__"] = VoidPtr.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_VoidPtr___destroy___0(self);
};

// Interface: btVector3
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btVector3(x, y, z) {
  if (x && typeof x === "object") x = x.ptr;
  if (y && typeof y === "object") y = y.ptr;
  if (z && typeof z === "object") z = z.ptr;
  if (x === undefined) {
    this.ptr = _emscripten_bind_btVector3_btVector3_0();
    getCache(btVector3)[this.ptr] = this;
    return;
  }
  if (y === undefined) {
    this.ptr = _emscripten_bind_btVector3_btVector3_1(x);
    getCache(btVector3)[this.ptr] = this;
    return;
  }
  if (z === undefined) {
    this.ptr = _emscripten_bind_btVector3_btVector3_2(x, y);
    getCache(btVector3)[this.ptr] = this;
    return;
  }
  this.ptr = _emscripten_bind_btVector3_btVector3_3(x, y, z);
  getCache(btVector3)[this.ptr] = this;
}

btVector3.prototype = Object.create(WrapperObject.prototype);

btVector3.prototype.constructor = btVector3;

btVector3.prototype.__class__ = btVector3;

btVector3.__cache__ = {};

Module["btVector3"] = btVector3;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btVector3.prototype["length"] = btVector3.prototype.length = function() {
  var self = this.ptr;
  return _emscripten_bind_btVector3_length_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btVector3.prototype["x"] = btVector3.prototype.x = function() {
  var self = this.ptr;
  return _emscripten_bind_btVector3_x_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btVector3.prototype["y"] = btVector3.prototype.y = function() {
  var self = this.ptr;
  return _emscripten_bind_btVector3_y_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btVector3.prototype["z"] = btVector3.prototype.z = function() {
  var self = this.ptr;
  return _emscripten_bind_btVector3_z_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btVector3.prototype["setValue"] = btVector3.prototype.setValue = function(x, y, z) {
  var self = this.ptr;
  if (x && typeof x === "object") x = x.ptr;
  if (y && typeof y === "object") y = y.ptr;
  if (z && typeof z === "object") z = z.ptr;
  _emscripten_bind_btVector3_setValue_3(self, x, y, z);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btVector3.prototype["__destroy__"] = btVector3.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btVector3___destroy___0(self);
};

// Interface: btQuaternion
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btQuaternion(x, y, z, w) {
  if (x && typeof x === "object") x = x.ptr;
  if (y && typeof y === "object") y = y.ptr;
  if (z && typeof z === "object") z = z.ptr;
  if (w && typeof w === "object") w = w.ptr;
  this.ptr = _emscripten_bind_btQuaternion_btQuaternion_4(x, y, z, w);
  getCache(btQuaternion)[this.ptr] = this;
}

btQuaternion.prototype = Object.create(WrapperObject.prototype);

btQuaternion.prototype.constructor = btQuaternion;

btQuaternion.prototype.__class__ = btQuaternion;

btQuaternion.__cache__ = {};

Module["btQuaternion"] = btQuaternion;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btQuaternion.prototype["setValue"] = btQuaternion.prototype.setValue = function(x, y, z, w) {
  var self = this.ptr;
  if (x && typeof x === "object") x = x.ptr;
  if (y && typeof y === "object") y = y.ptr;
  if (z && typeof z === "object") z = z.ptr;
  if (w && typeof w === "object") w = w.ptr;
  _emscripten_bind_btQuaternion_setValue_4(self, x, y, z, w);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btQuaternion.prototype["setEulerZYX"] = btQuaternion.prototype.setEulerZYX = function(z, y, x) {
  var self = this.ptr;
  if (z && typeof z === "object") z = z.ptr;
  if (y && typeof y === "object") y = y.ptr;
  if (x && typeof x === "object") x = x.ptr;
  _emscripten_bind_btQuaternion_setEulerZYX_3(self, z, y, x);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btQuaternion.prototype["x"] = btQuaternion.prototype.x = function() {
  var self = this.ptr;
  return _emscripten_bind_btQuaternion_x_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btQuaternion.prototype["y"] = btQuaternion.prototype.y = function() {
  var self = this.ptr;
  return _emscripten_bind_btQuaternion_y_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btQuaternion.prototype["z"] = btQuaternion.prototype.z = function() {
  var self = this.ptr;
  return _emscripten_bind_btQuaternion_z_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btQuaternion.prototype["w"] = btQuaternion.prototype.w = function() {
  var self = this.ptr;
  return _emscripten_bind_btQuaternion_w_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btQuaternion.prototype["__destroy__"] = btQuaternion.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btQuaternion___destroy___0(self);
};

// Interface: btTransform
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btTransform(q, v) {
  if (q && typeof q === "object") q = q.ptr;
  if (v && typeof v === "object") v = v.ptr;
  if (q === undefined) {
    this.ptr = _emscripten_bind_btTransform_btTransform_0();
    getCache(btTransform)[this.ptr] = this;
    return;
  }
  if (v === undefined) {
    this.ptr = _emscripten_bind_btTransform_btTransform_1(q);
    getCache(btTransform)[this.ptr] = this;
    return;
  }
  this.ptr = _emscripten_bind_btTransform_btTransform_2(q, v);
  getCache(btTransform)[this.ptr] = this;
}

btTransform.prototype = Object.create(WrapperObject.prototype);

btTransform.prototype.constructor = btTransform;

btTransform.prototype.__class__ = btTransform;

btTransform.__cache__ = {};

Module["btTransform"] = btTransform;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTransform.prototype["setIdentity"] = btTransform.prototype.setIdentity = function() {
  var self = this.ptr;
  _emscripten_bind_btTransform_setIdentity_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTransform.prototype["setOrigin"] = btTransform.prototype.setOrigin = function(origin) {
  var self = this.ptr;
  if (origin && typeof origin === "object") origin = origin.ptr;
  _emscripten_bind_btTransform_setOrigin_1(self, origin);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTransform.prototype["setRotation"] = btTransform.prototype.setRotation = function(rotation) {
  var self = this.ptr;
  if (rotation && typeof rotation === "object") rotation = rotation.ptr;
  _emscripten_bind_btTransform_setRotation_1(self, rotation);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTransform.prototype["getOrigin"] = btTransform.prototype.getOrigin = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btTransform_getOrigin_0(self), btVector3);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTransform.prototype["setEulerZYX"] = btTransform.prototype.setEulerZYX = function(x, y, z) {
  var self = this.ptr;
  if (x && typeof x === "object") x = x.ptr;
  if (y && typeof y === "object") y = y.ptr;
  if (z && typeof z === "object") z = z.ptr;
  _emscripten_bind_btTransform_setEulerZYX_3(self, x, y, z);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTransform.prototype["getRotation"] = btTransform.prototype.getRotation = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btTransform_getRotation_0(self), btQuaternion);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTransform.prototype["__destroy__"] = btTransform.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btTransform___destroy___0(self);
};

// Interface: btDefaultMotionState
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btDefaultMotionState(startTrans, centerOfMassOffset) {
  if (startTrans && typeof startTrans === "object") startTrans = startTrans.ptr;
  if (centerOfMassOffset && typeof centerOfMassOffset === "object") centerOfMassOffset = centerOfMassOffset.ptr;
  if (startTrans === undefined) {
    this.ptr = _emscripten_bind_btDefaultMotionState_btDefaultMotionState_0();
    getCache(btDefaultMotionState)[this.ptr] = this;
    return;
  }
  if (centerOfMassOffset === undefined) {
    this.ptr = _emscripten_bind_btDefaultMotionState_btDefaultMotionState_1(startTrans);
    getCache(btDefaultMotionState)[this.ptr] = this;
    return;
  }
  this.ptr = _emscripten_bind_btDefaultMotionState_btDefaultMotionState_2(startTrans, centerOfMassOffset);
  getCache(btDefaultMotionState)[this.ptr] = this;
}

btDefaultMotionState.prototype = Object.create(btMotionState.prototype);

btDefaultMotionState.prototype.constructor = btDefaultMotionState;

btDefaultMotionState.prototype.__class__ = btDefaultMotionState;

btDefaultMotionState.__cache__ = {};

Module["btDefaultMotionState"] = btDefaultMotionState;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDefaultMotionState.prototype["getWorldTransform"] = btDefaultMotionState.prototype.getWorldTransform = function(worldTrans) {
  var self = this.ptr;
  if (worldTrans && typeof worldTrans === "object") worldTrans = worldTrans.ptr;
  _emscripten_bind_btDefaultMotionState_getWorldTransform_1(self, worldTrans);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDefaultMotionState.prototype["setWorldTransform"] = btDefaultMotionState.prototype.setWorldTransform = function(worldTrans) {
  var self = this.ptr;
  if (worldTrans && typeof worldTrans === "object") worldTrans = worldTrans.ptr;
  _emscripten_bind_btDefaultMotionState_setWorldTransform_1(self, worldTrans);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDefaultMotionState.prototype["__destroy__"] = btDefaultMotionState.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btDefaultMotionState___destroy___0(self);
};

// Interface: btConvexShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btConvexShape() {
  throw "cannot construct a btConvexShape, no constructor in IDL";
}

btConvexShape.prototype = Object.create(btCollisionShape.prototype);

btConvexShape.prototype.constructor = btConvexShape;

btConvexShape.prototype.__class__ = btConvexShape;

btConvexShape.__cache__ = {};

Module["btConvexShape"] = btConvexShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btConvexShape.prototype["setLocalScaling"] = btConvexShape.prototype.setLocalScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btConvexShape_setLocalScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btConvexShape.prototype["__destroy__"] = btConvexShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btConvexShape___destroy___0(self);
};

// Interface: btBoxShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btBoxShape(boxHalfExtents) {
  if (boxHalfExtents && typeof boxHalfExtents === "object") boxHalfExtents = boxHalfExtents.ptr;
  this.ptr = _emscripten_bind_btBoxShape_btBoxShape_1(boxHalfExtents);
  getCache(btBoxShape)[this.ptr] = this;
}

btBoxShape.prototype = Object.create(btCollisionShape.prototype);

btBoxShape.prototype.constructor = btBoxShape;

btBoxShape.prototype.__class__ = btBoxShape;

btBoxShape.__cache__ = {};

Module["btBoxShape"] = btBoxShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBoxShape.prototype["setLocalScaling"] = btBoxShape.prototype.setLocalScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btBoxShape_setLocalScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBoxShape.prototype["__destroy__"] = btBoxShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btBoxShape___destroy___0(self);
};

// Interface: btCapsuleShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btCapsuleShape(radius, height) {
  if (radius && typeof radius === "object") radius = radius.ptr;
  if (height && typeof height === "object") height = height.ptr;
  this.ptr = _emscripten_bind_btCapsuleShape_btCapsuleShape_2(radius, height);
  getCache(btCapsuleShape)[this.ptr] = this;
}

btCapsuleShape.prototype = Object.create(btCollisionShape.prototype);

btCapsuleShape.prototype.constructor = btCapsuleShape;

btCapsuleShape.prototype.__class__ = btCapsuleShape;

btCapsuleShape.__cache__ = {};

Module["btCapsuleShape"] = btCapsuleShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCapsuleShape.prototype["setLocalScaling"] = btCapsuleShape.prototype.setLocalScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btCapsuleShape_setLocalScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCapsuleShape.prototype["__destroy__"] = btCapsuleShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btCapsuleShape___destroy___0(self);
};

// Interface: btCylinderShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btCylinderShape(halfExtents) {
  if (halfExtents && typeof halfExtents === "object") halfExtents = halfExtents.ptr;
  this.ptr = _emscripten_bind_btCylinderShape_btCylinderShape_1(halfExtents);
  getCache(btCylinderShape)[this.ptr] = this;
}

btCylinderShape.prototype = Object.create(btCollisionShape.prototype);

btCylinderShape.prototype.constructor = btCylinderShape;

btCylinderShape.prototype.__class__ = btCylinderShape;

btCylinderShape.__cache__ = {};

Module["btCylinderShape"] = btCylinderShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCylinderShape.prototype["setLocalScaling"] = btCylinderShape.prototype.setLocalScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btCylinderShape_setLocalScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCylinderShape.prototype["__destroy__"] = btCylinderShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btCylinderShape___destroy___0(self);
};

// Interface: btSphereShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btSphereShape(radius) {
  if (radius && typeof radius === "object") radius = radius.ptr;
  this.ptr = _emscripten_bind_btSphereShape_btSphereShape_1(radius);
  getCache(btSphereShape)[this.ptr] = this;
}

btSphereShape.prototype = Object.create(btCollisionShape.prototype);

btSphereShape.prototype.constructor = btSphereShape;

btSphereShape.prototype.__class__ = btSphereShape;

btSphereShape.__cache__ = {};

Module["btSphereShape"] = btSphereShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btSphereShape.prototype["setLocalScaling"] = btSphereShape.prototype.setLocalScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btSphereShape_setLocalScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btSphereShape.prototype["__destroy__"] = btSphereShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btSphereShape___destroy___0(self);
};

// Interface: btConeShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btConeShape(radius, height) {
  if (radius && typeof radius === "object") radius = radius.ptr;
  if (height && typeof height === "object") height = height.ptr;
  this.ptr = _emscripten_bind_btConeShape_btConeShape_2(radius, height);
  getCache(btConeShape)[this.ptr] = this;
}

btConeShape.prototype = Object.create(btCollisionShape.prototype);

btConeShape.prototype.constructor = btConeShape;

btConeShape.prototype.__class__ = btConeShape;

btConeShape.__cache__ = {};

Module["btConeShape"] = btConeShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btConeShape.prototype["setLocalScaling"] = btConeShape.prototype.setLocalScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btConeShape_setLocalScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btConeShape.prototype["__destroy__"] = btConeShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btConeShape___destroy___0(self);
};

// Interface: btConvexHullShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btConvexHullShape(points, numPoints) {
  ensureCache.prepare();
  if (typeof points == "object") {
    points = ensureFloat32(points);
  }
  if (numPoints && typeof numPoints === "object") numPoints = numPoints.ptr;
  if (points === undefined) {
    this.ptr = _emscripten_bind_btConvexHullShape_btConvexHullShape_0();
    getCache(btConvexHullShape)[this.ptr] = this;
    return;
  }
  if (numPoints === undefined) {
    this.ptr = _emscripten_bind_btConvexHullShape_btConvexHullShape_1(points);
    getCache(btConvexHullShape)[this.ptr] = this;
    return;
  }
  this.ptr = _emscripten_bind_btConvexHullShape_btConvexHullShape_2(points, numPoints);
  getCache(btConvexHullShape)[this.ptr] = this;
}

btConvexHullShape.prototype = Object.create(btCollisionShape.prototype);

btConvexHullShape.prototype.constructor = btConvexHullShape;

btConvexHullShape.prototype.__class__ = btConvexHullShape;

btConvexHullShape.__cache__ = {};

Module["btConvexHullShape"] = btConvexHullShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btConvexHullShape.prototype["addPoint"] = btConvexHullShape.prototype.addPoint = function(point, recalculateLocalAABB) {
  var self = this.ptr;
  if (point && typeof point === "object") point = point.ptr;
  if (recalculateLocalAABB && typeof recalculateLocalAABB === "object") recalculateLocalAABB = recalculateLocalAABB.ptr;
  if (recalculateLocalAABB === undefined) {
    _emscripten_bind_btConvexHullShape_addPoint_1(self, point);
    return;
  }
  _emscripten_bind_btConvexHullShape_addPoint_2(self, point, recalculateLocalAABB);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btConvexHullShape.prototype["setLocalScaling"] = btConvexHullShape.prototype.setLocalScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btConvexHullShape_setLocalScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btConvexHullShape.prototype["__destroy__"] = btConvexHullShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btConvexHullShape___destroy___0(self);
};

// Interface: btCompoundShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btCompoundShape(enableDynamicAabbTree) {
  if (enableDynamicAabbTree && typeof enableDynamicAabbTree === "object") enableDynamicAabbTree = enableDynamicAabbTree.ptr;
  if (enableDynamicAabbTree === undefined) {
    this.ptr = _emscripten_bind_btCompoundShape_btCompoundShape_0();
    getCache(btCompoundShape)[this.ptr] = this;
    return;
  }
  this.ptr = _emscripten_bind_btCompoundShape_btCompoundShape_1(enableDynamicAabbTree);
  getCache(btCompoundShape)[this.ptr] = this;
}

btCompoundShape.prototype = Object.create(btCollisionShape.prototype);

btCompoundShape.prototype.constructor = btCompoundShape;

btCompoundShape.prototype.__class__ = btCompoundShape;

btCompoundShape.__cache__ = {};

Module["btCompoundShape"] = btCompoundShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCompoundShape.prototype["addChildShape"] = btCompoundShape.prototype.addChildShape = function(localTransform, shape) {
  var self = this.ptr;
  if (localTransform && typeof localTransform === "object") localTransform = localTransform.ptr;
  if (shape && typeof shape === "object") shape = shape.ptr;
  _emscripten_bind_btCompoundShape_addChildShape_2(self, localTransform, shape);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCompoundShape.prototype["setLocalScaling"] = btCompoundShape.prototype.setLocalScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btCompoundShape_setLocalScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCompoundShape.prototype["__destroy__"] = btCompoundShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btCompoundShape___destroy___0(self);
};

// Interface: btTriangleMesh
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btTriangleMesh(use32bitIndices, use4componentVertices) {
  if (use32bitIndices && typeof use32bitIndices === "object") use32bitIndices = use32bitIndices.ptr;
  if (use4componentVertices && typeof use4componentVertices === "object") use4componentVertices = use4componentVertices.ptr;
  if (use32bitIndices === undefined) {
    this.ptr = _emscripten_bind_btTriangleMesh_btTriangleMesh_0();
    getCache(btTriangleMesh)[this.ptr] = this;
    return;
  }
  if (use4componentVertices === undefined) {
    this.ptr = _emscripten_bind_btTriangleMesh_btTriangleMesh_1(use32bitIndices);
    getCache(btTriangleMesh)[this.ptr] = this;
    return;
  }
  this.ptr = _emscripten_bind_btTriangleMesh_btTriangleMesh_2(use32bitIndices, use4componentVertices);
  getCache(btTriangleMesh)[this.ptr] = this;
}

btTriangleMesh.prototype = Object.create(btStridingMeshInterface.prototype);

btTriangleMesh.prototype.constructor = btTriangleMesh;

btTriangleMesh.prototype.__class__ = btTriangleMesh;

btTriangleMesh.__cache__ = {};

Module["btTriangleMesh"] = btTriangleMesh;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleMesh.prototype["addTriangle"] = btTriangleMesh.prototype.addTriangle = function(vertex0, vertex1, vertex2, removeDuplicateVertices) {
  var self = this.ptr;
  if (vertex0 && typeof vertex0 === "object") vertex0 = vertex0.ptr;
  if (vertex1 && typeof vertex1 === "object") vertex1 = vertex1.ptr;
  if (vertex2 && typeof vertex2 === "object") vertex2 = vertex2.ptr;
  if (removeDuplicateVertices && typeof removeDuplicateVertices === "object") removeDuplicateVertices = removeDuplicateVertices.ptr;
  if (removeDuplicateVertices === undefined) {
    _emscripten_bind_btTriangleMesh_addTriangle_3(self, vertex0, vertex1, vertex2);
    return;
  }
  _emscripten_bind_btTriangleMesh_addTriangle_4(self, vertex0, vertex1, vertex2, removeDuplicateVertices);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleMesh.prototype["preallocateIndices"] = btTriangleMesh.prototype.preallocateIndices = function(numindices) {
  var self = this.ptr;
  if (numindices && typeof numindices === "object") numindices = numindices.ptr;
  _emscripten_bind_btTriangleMesh_preallocateIndices_1(self, numindices);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleMesh.prototype["preallocateVertices"] = btTriangleMesh.prototype.preallocateVertices = function(numverts) {
  var self = this.ptr;
  if (numverts && typeof numverts === "object") numverts = numverts.ptr;
  _emscripten_bind_btTriangleMesh_preallocateVertices_1(self, numverts);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleMesh.prototype["__destroy__"] = btTriangleMesh.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btTriangleMesh___destroy___0(self);
};

// Interface: btTriangleIndexVertexArrayWrapper
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btTriangleIndexVertexArrayWrapper(numTriangles, triangleIndexBase, triangleIndexStride, numVertices, vertexBase, vertexStride) {
  if (numTriangles && typeof numTriangles === "object") numTriangles = numTriangles.ptr;
  if (triangleIndexBase && typeof triangleIndexBase === "object") triangleIndexBase = triangleIndexBase.ptr;
  if (triangleIndexStride && typeof triangleIndexStride === "object") triangleIndexStride = triangleIndexStride.ptr;
  if (numVertices && typeof numVertices === "object") numVertices = numVertices.ptr;
  if (vertexBase && typeof vertexBase === "object") vertexBase = vertexBase.ptr;
  if (vertexStride && typeof vertexStride === "object") vertexStride = vertexStride.ptr;
  this.ptr = _emscripten_bind_btTriangleIndexVertexArrayWrapper_btTriangleIndexVertexArrayWrapper_6(numTriangles, triangleIndexBase, triangleIndexStride, numVertices, vertexBase, vertexStride);
  getCache(btTriangleIndexVertexArrayWrapper)[this.ptr] = this;
}

btTriangleIndexVertexArrayWrapper.prototype = Object.create(btStridingMeshInterface.prototype);

btTriangleIndexVertexArrayWrapper.prototype.constructor = btTriangleIndexVertexArrayWrapper;

btTriangleIndexVertexArrayWrapper.prototype.__class__ = btTriangleIndexVertexArrayWrapper;

btTriangleIndexVertexArrayWrapper.__cache__ = {};

Module["btTriangleIndexVertexArrayWrapper"] = btTriangleIndexVertexArrayWrapper;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleIndexVertexArrayWrapper.prototype["__destroy__"] = btTriangleIndexVertexArrayWrapper.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btTriangleIndexVertexArrayWrapper___destroy___0(self);
};

// Interface: btTriangleInfoMap
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btTriangleInfoMap() {
  this.ptr = _emscripten_bind_btTriangleInfoMap_btTriangleInfoMap_0();
  getCache(btTriangleInfoMap)[this.ptr] = this;
}

btTriangleInfoMap.prototype = Object.create(WrapperObject.prototype);

btTriangleInfoMap.prototype.constructor = btTriangleInfoMap;

btTriangleInfoMap.prototype.__class__ = btTriangleInfoMap;

btTriangleInfoMap.__cache__ = {};

Module["btTriangleInfoMap"] = btTriangleInfoMap;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["get_m_convexEpsilon"] = btTriangleInfoMap.prototype.get_m_convexEpsilon = function() {
  var self = this.ptr;
  return _emscripten_bind_btTriangleInfoMap_get_m_convexEpsilon_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["set_m_convexEpsilon"] = btTriangleInfoMap.prototype.set_m_convexEpsilon = function(arg0) {
  var self = this.ptr;
  if (arg0 && typeof arg0 === "object") arg0 = arg0.ptr;
  _emscripten_bind_btTriangleInfoMap_set_m_convexEpsilon_1(self, arg0);
};

/** @suppress {checkTypes} */ Object.defineProperty(btTriangleInfoMap.prototype, "m_convexEpsilon", {
  get: btTriangleInfoMap.prototype.get_m_convexEpsilon,
  set: btTriangleInfoMap.prototype.set_m_convexEpsilon
});

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["get_m_planarEpsilon"] = btTriangleInfoMap.prototype.get_m_planarEpsilon = function() {
  var self = this.ptr;
  return _emscripten_bind_btTriangleInfoMap_get_m_planarEpsilon_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["set_m_planarEpsilon"] = btTriangleInfoMap.prototype.set_m_planarEpsilon = function(arg0) {
  var self = this.ptr;
  if (arg0 && typeof arg0 === "object") arg0 = arg0.ptr;
  _emscripten_bind_btTriangleInfoMap_set_m_planarEpsilon_1(self, arg0);
};

/** @suppress {checkTypes} */ Object.defineProperty(btTriangleInfoMap.prototype, "m_planarEpsilon", {
  get: btTriangleInfoMap.prototype.get_m_planarEpsilon,
  set: btTriangleInfoMap.prototype.set_m_planarEpsilon
});

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["get_m_equalVertexThreshold"] = btTriangleInfoMap.prototype.get_m_equalVertexThreshold = function() {
  var self = this.ptr;
  return _emscripten_bind_btTriangleInfoMap_get_m_equalVertexThreshold_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["set_m_equalVertexThreshold"] = btTriangleInfoMap.prototype.set_m_equalVertexThreshold = function(arg0) {
  var self = this.ptr;
  if (arg0 && typeof arg0 === "object") arg0 = arg0.ptr;
  _emscripten_bind_btTriangleInfoMap_set_m_equalVertexThreshold_1(self, arg0);
};

/** @suppress {checkTypes} */ Object.defineProperty(btTriangleInfoMap.prototype, "m_equalVertexThreshold", {
  get: btTriangleInfoMap.prototype.get_m_equalVertexThreshold,
  set: btTriangleInfoMap.prototype.set_m_equalVertexThreshold
});

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["get_m_edgeDistanceThreshold"] = btTriangleInfoMap.prototype.get_m_edgeDistanceThreshold = function() {
  var self = this.ptr;
  return _emscripten_bind_btTriangleInfoMap_get_m_edgeDistanceThreshold_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["set_m_edgeDistanceThreshold"] = btTriangleInfoMap.prototype.set_m_edgeDistanceThreshold = function(arg0) {
  var self = this.ptr;
  if (arg0 && typeof arg0 === "object") arg0 = arg0.ptr;
  _emscripten_bind_btTriangleInfoMap_set_m_edgeDistanceThreshold_1(self, arg0);
};

/** @suppress {checkTypes} */ Object.defineProperty(btTriangleInfoMap.prototype, "m_edgeDistanceThreshold", {
  get: btTriangleInfoMap.prototype.get_m_edgeDistanceThreshold,
  set: btTriangleInfoMap.prototype.set_m_edgeDistanceThreshold
});

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["get_m_maxEdgeAngleThreshold"] = btTriangleInfoMap.prototype.get_m_maxEdgeAngleThreshold = function() {
  var self = this.ptr;
  return _emscripten_bind_btTriangleInfoMap_get_m_maxEdgeAngleThreshold_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["set_m_maxEdgeAngleThreshold"] = btTriangleInfoMap.prototype.set_m_maxEdgeAngleThreshold = function(arg0) {
  var self = this.ptr;
  if (arg0 && typeof arg0 === "object") arg0 = arg0.ptr;
  _emscripten_bind_btTriangleInfoMap_set_m_maxEdgeAngleThreshold_1(self, arg0);
};

/** @suppress {checkTypes} */ Object.defineProperty(btTriangleInfoMap.prototype, "m_maxEdgeAngleThreshold", {
  get: btTriangleInfoMap.prototype.get_m_maxEdgeAngleThreshold,
  set: btTriangleInfoMap.prototype.set_m_maxEdgeAngleThreshold
});

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["get_m_zeroAreaThreshold"] = btTriangleInfoMap.prototype.get_m_zeroAreaThreshold = function() {
  var self = this.ptr;
  return _emscripten_bind_btTriangleInfoMap_get_m_zeroAreaThreshold_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["set_m_zeroAreaThreshold"] = btTriangleInfoMap.prototype.set_m_zeroAreaThreshold = function(arg0) {
  var self = this.ptr;
  if (arg0 && typeof arg0 === "object") arg0 = arg0.ptr;
  _emscripten_bind_btTriangleInfoMap_set_m_zeroAreaThreshold_1(self, arg0);
};

/** @suppress {checkTypes} */ Object.defineProperty(btTriangleInfoMap.prototype, "m_zeroAreaThreshold", {
  get: btTriangleInfoMap.prototype.get_m_zeroAreaThreshold,
  set: btTriangleInfoMap.prototype.set_m_zeroAreaThreshold
});

/** @suppress {undefinedVars, duplicate} @this{Object} */ btTriangleInfoMap.prototype["__destroy__"] = btTriangleInfoMap.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btTriangleInfoMap___destroy___0(self);
};

// Interface: btBvhTriangleMeshShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btBvhTriangleMeshShape(meshInterface, useQuantizedAabbCompression, buildBvh) {
  if (meshInterface && typeof meshInterface === "object") meshInterface = meshInterface.ptr;
  if (useQuantizedAabbCompression && typeof useQuantizedAabbCompression === "object") useQuantizedAabbCompression = useQuantizedAabbCompression.ptr;
  if (buildBvh && typeof buildBvh === "object") buildBvh = buildBvh.ptr;
  if (buildBvh === undefined) {
    this.ptr = _emscripten_bind_btBvhTriangleMeshShape_btBvhTriangleMeshShape_2(meshInterface, useQuantizedAabbCompression);
    getCache(btBvhTriangleMeshShape)[this.ptr] = this;
    return;
  }
  this.ptr = _emscripten_bind_btBvhTriangleMeshShape_btBvhTriangleMeshShape_3(meshInterface, useQuantizedAabbCompression, buildBvh);
  getCache(btBvhTriangleMeshShape)[this.ptr] = this;
}

btBvhTriangleMeshShape.prototype = Object.create(WrapperObject.prototype);

btBvhTriangleMeshShape.prototype.constructor = btBvhTriangleMeshShape;

btBvhTriangleMeshShape.prototype.__class__ = btBvhTriangleMeshShape;

btBvhTriangleMeshShape.__cache__ = {};

Module["btBvhTriangleMeshShape"] = btBvhTriangleMeshShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBvhTriangleMeshShape.prototype["setTriangleInfoMap"] = btBvhTriangleMeshShape.prototype.setTriangleInfoMap = function(triangleInfoMap) {
  var self = this.ptr;
  if (triangleInfoMap && typeof triangleInfoMap === "object") triangleInfoMap = triangleInfoMap.ptr;
  _emscripten_bind_btBvhTriangleMeshShape_setTriangleInfoMap_1(self, triangleInfoMap);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBvhTriangleMeshShape.prototype["getTriangleInfoMap"] = btBvhTriangleMeshShape.prototype.getTriangleInfoMap = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btBvhTriangleMeshShape_getTriangleInfoMap_0(self), btTriangleInfoMap);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBvhTriangleMeshShape.prototype["__destroy__"] = btBvhTriangleMeshShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btBvhTriangleMeshShape___destroy___0(self);
};

// Interface: btScaledBvhTriangleMeshShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btScaledBvhTriangleMeshShape(childShape, localScaling) {
  if (childShape && typeof childShape === "object") childShape = childShape.ptr;
  if (localScaling && typeof localScaling === "object") localScaling = localScaling.ptr;
  this.ptr = _emscripten_bind_btScaledBvhTriangleMeshShape_btScaledBvhTriangleMeshShape_2(childShape, localScaling);
  getCache(btScaledBvhTriangleMeshShape)[this.ptr] = this;
}

btScaledBvhTriangleMeshShape.prototype = Object.create(btConcaveShape.prototype);

btScaledBvhTriangleMeshShape.prototype.constructor = btScaledBvhTriangleMeshShape;

btScaledBvhTriangleMeshShape.prototype.__class__ = btScaledBvhTriangleMeshShape;

btScaledBvhTriangleMeshShape.__cache__ = {};

Module["btScaledBvhTriangleMeshShape"] = btScaledBvhTriangleMeshShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btScaledBvhTriangleMeshShape.prototype["getChildShape"] = btScaledBvhTriangleMeshShape.prototype.getChildShape = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btScaledBvhTriangleMeshShape_getChildShape_0(self), btBvhTriangleMeshShape);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btScaledBvhTriangleMeshShape.prototype["setLocalScaling"] = btScaledBvhTriangleMeshShape.prototype.setLocalScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btScaledBvhTriangleMeshShape_setLocalScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btScaledBvhTriangleMeshShape.prototype["__destroy__"] = btScaledBvhTriangleMeshShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btScaledBvhTriangleMeshShape___destroy___0(self);
};

// Interface: btInternalEdgeUtility
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btInternalEdgeUtility() {
  throw "cannot construct a btInternalEdgeUtility, no constructor in IDL";
}

btInternalEdgeUtility.prototype = Object.create(WrapperObject.prototype);

btInternalEdgeUtility.prototype.constructor = btInternalEdgeUtility;

btInternalEdgeUtility.prototype.__class__ = btInternalEdgeUtility;

btInternalEdgeUtility.__cache__ = {};

Module["btInternalEdgeUtility"] = btInternalEdgeUtility;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btInternalEdgeUtility.prototype["btGenerateInternalEdgeInfo"] = btInternalEdgeUtility.prototype.btGenerateInternalEdgeInfo = function(trimeshShape, triangleInfoMap) {
  if (trimeshShape && typeof trimeshShape === "object") trimeshShape = trimeshShape.ptr;
  if (triangleInfoMap && typeof triangleInfoMap === "object") triangleInfoMap = triangleInfoMap.ptr;
  _emscripten_bind_btInternalEdgeUtility_btGenerateInternalEdgeInfo_2(trimeshShape, triangleInfoMap);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btInternalEdgeUtility.prototype["__destroy__"] = btInternalEdgeUtility.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btInternalEdgeUtility___destroy___0(self);
};

// Interface: btHeightfieldTerrainShape
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btHeightfieldTerrainShape(heightStickWidth, heightStickLength, heightfieldData, heightScale, minHeight, maxHeight, upAxis, hdt, flipQuadEdges) {
  if (heightStickWidth && typeof heightStickWidth === "object") heightStickWidth = heightStickWidth.ptr;
  if (heightStickLength && typeof heightStickLength === "object") heightStickLength = heightStickLength.ptr;
  if (heightfieldData && typeof heightfieldData === "object") heightfieldData = heightfieldData.ptr;
  if (heightScale && typeof heightScale === "object") heightScale = heightScale.ptr;
  if (minHeight && typeof minHeight === "object") minHeight = minHeight.ptr;
  if (maxHeight && typeof maxHeight === "object") maxHeight = maxHeight.ptr;
  if (upAxis && typeof upAxis === "object") upAxis = upAxis.ptr;
  if (hdt && typeof hdt === "object") hdt = hdt.ptr;
  if (flipQuadEdges && typeof flipQuadEdges === "object") flipQuadEdges = flipQuadEdges.ptr;
  this.ptr = _emscripten_bind_btHeightfieldTerrainShape_btHeightfieldTerrainShape_9(heightStickWidth, heightStickLength, heightfieldData, heightScale, minHeight, maxHeight, upAxis, hdt, flipQuadEdges);
  getCache(btHeightfieldTerrainShape)[this.ptr] = this;
}

btHeightfieldTerrainShape.prototype = Object.create(btConcaveShape.prototype);

btHeightfieldTerrainShape.prototype.constructor = btHeightfieldTerrainShape;

btHeightfieldTerrainShape.prototype.__class__ = btHeightfieldTerrainShape;

btHeightfieldTerrainShape.__cache__ = {};

Module["btHeightfieldTerrainShape"] = btHeightfieldTerrainShape;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btHeightfieldTerrainShape.prototype["setMargin"] = btHeightfieldTerrainShape.prototype.setMargin = function(margin) {
  var self = this.ptr;
  if (margin && typeof margin === "object") margin = margin.ptr;
  _emscripten_bind_btHeightfieldTerrainShape_setMargin_1(self, margin);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btHeightfieldTerrainShape.prototype["getMargin"] = btHeightfieldTerrainShape.prototype.getMargin = function() {
  var self = this.ptr;
  return _emscripten_bind_btHeightfieldTerrainShape_getMargin_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btHeightfieldTerrainShape.prototype["setLocalScaling"] = btHeightfieldTerrainShape.prototype.setLocalScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btHeightfieldTerrainShape_setLocalScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btHeightfieldTerrainShape.prototype["__destroy__"] = btHeightfieldTerrainShape.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btHeightfieldTerrainShape___destroy___0(self);
};

// Interface: btDefaultCollisionConstructionInfo
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btDefaultCollisionConstructionInfo() {
  this.ptr = _emscripten_bind_btDefaultCollisionConstructionInfo_btDefaultCollisionConstructionInfo_0();
  getCache(btDefaultCollisionConstructionInfo)[this.ptr] = this;
}

btDefaultCollisionConstructionInfo.prototype = Object.create(WrapperObject.prototype);

btDefaultCollisionConstructionInfo.prototype.constructor = btDefaultCollisionConstructionInfo;

btDefaultCollisionConstructionInfo.prototype.__class__ = btDefaultCollisionConstructionInfo;

btDefaultCollisionConstructionInfo.__cache__ = {};

Module["btDefaultCollisionConstructionInfo"] = btDefaultCollisionConstructionInfo;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDefaultCollisionConstructionInfo.prototype["__destroy__"] = btDefaultCollisionConstructionInfo.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btDefaultCollisionConstructionInfo___destroy___0(self);
};

// Interface: btDefaultCollisionConfiguration
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btDefaultCollisionConfiguration(info) {
  if (info && typeof info === "object") info = info.ptr;
  if (info === undefined) {
    this.ptr = _emscripten_bind_btDefaultCollisionConfiguration_btDefaultCollisionConfiguration_0();
    getCache(btDefaultCollisionConfiguration)[this.ptr] = this;
    return;
  }
  this.ptr = _emscripten_bind_btDefaultCollisionConfiguration_btDefaultCollisionConfiguration_1(info);
  getCache(btDefaultCollisionConfiguration)[this.ptr] = this;
}

btDefaultCollisionConfiguration.prototype = Object.create(WrapperObject.prototype);

btDefaultCollisionConfiguration.prototype.constructor = btDefaultCollisionConfiguration;

btDefaultCollisionConfiguration.prototype.__class__ = btDefaultCollisionConfiguration;

btDefaultCollisionConfiguration.__cache__ = {};

Module["btDefaultCollisionConfiguration"] = btDefaultCollisionConfiguration;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDefaultCollisionConfiguration.prototype["__destroy__"] = btDefaultCollisionConfiguration.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btDefaultCollisionConfiguration___destroy___0(self);
};

// Interface: btDispatcher
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btDispatcher() {
  throw "cannot construct a btDispatcher, no constructor in IDL";
}

btDispatcher.prototype = Object.create(WrapperObject.prototype);

btDispatcher.prototype.constructor = btDispatcher;

btDispatcher.prototype.__class__ = btDispatcher;

btDispatcher.__cache__ = {};

Module["btDispatcher"] = btDispatcher;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDispatcher.prototype["__destroy__"] = btDispatcher.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btDispatcher___destroy___0(self);
};

// Interface: btCollisionDispatcher
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btCollisionDispatcher(conf) {
  if (conf && typeof conf === "object") conf = conf.ptr;
  this.ptr = _emscripten_bind_btCollisionDispatcher_btCollisionDispatcher_1(conf);
  getCache(btCollisionDispatcher)[this.ptr] = this;
}

btCollisionDispatcher.prototype = Object.create(WrapperObject.prototype);

btCollisionDispatcher.prototype.constructor = btCollisionDispatcher;

btCollisionDispatcher.prototype.__class__ = btCollisionDispatcher;

btCollisionDispatcher.__cache__ = {};

Module["btCollisionDispatcher"] = btCollisionDispatcher;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionDispatcher.prototype["__destroy__"] = btCollisionDispatcher.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btCollisionDispatcher___destroy___0(self);
};

// Interface: btOverlappingPairCallback
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btOverlappingPairCallback() {
  throw "cannot construct a btOverlappingPairCallback, no constructor in IDL";
}

btOverlappingPairCallback.prototype = Object.create(WrapperObject.prototype);

btOverlappingPairCallback.prototype.constructor = btOverlappingPairCallback;

btOverlappingPairCallback.prototype.__class__ = btOverlappingPairCallback;

btOverlappingPairCallback.__cache__ = {};

Module["btOverlappingPairCallback"] = btOverlappingPairCallback;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btOverlappingPairCallback.prototype["__destroy__"] = btOverlappingPairCallback.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btOverlappingPairCallback___destroy___0(self);
};

// Interface: btOverlappingPairCache
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btOverlappingPairCache() {
  throw "cannot construct a btOverlappingPairCache, no constructor in IDL";
}

btOverlappingPairCache.prototype = Object.create(WrapperObject.prototype);

btOverlappingPairCache.prototype.constructor = btOverlappingPairCache;

btOverlappingPairCache.prototype.__class__ = btOverlappingPairCache;

btOverlappingPairCache.__cache__ = {};

Module["btOverlappingPairCache"] = btOverlappingPairCache;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btOverlappingPairCache.prototype["setInternalGhostPairCallback"] = btOverlappingPairCache.prototype.setInternalGhostPairCallback = function(ghostPairCallback) {
  var self = this.ptr;
  if (ghostPairCallback && typeof ghostPairCallback === "object") ghostPairCallback = ghostPairCallback.ptr;
  _emscripten_bind_btOverlappingPairCache_setInternalGhostPairCallback_1(self, ghostPairCallback);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btOverlappingPairCache.prototype["__destroy__"] = btOverlappingPairCache.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btOverlappingPairCache___destroy___0(self);
};

// Interface: btBroadphaseInterface
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btBroadphaseInterface() {
  throw "cannot construct a btBroadphaseInterface, no constructor in IDL";
}

btBroadphaseInterface.prototype = Object.create(WrapperObject.prototype);

btBroadphaseInterface.prototype.constructor = btBroadphaseInterface;

btBroadphaseInterface.prototype.__class__ = btBroadphaseInterface;

btBroadphaseInterface.__cache__ = {};

Module["btBroadphaseInterface"] = btBroadphaseInterface;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBroadphaseInterface.prototype["getOverlappingPairCache"] = btBroadphaseInterface.prototype.getOverlappingPairCache = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btBroadphaseInterface_getOverlappingPairCache_0(self), btOverlappingPairCache);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBroadphaseInterface.prototype["__destroy__"] = btBroadphaseInterface.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btBroadphaseInterface___destroy___0(self);
};

// Interface: btCollisionConfiguration
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btCollisionConfiguration() {
  throw "cannot construct a btCollisionConfiguration, no constructor in IDL";
}

btCollisionConfiguration.prototype = Object.create(WrapperObject.prototype);

btCollisionConfiguration.prototype.constructor = btCollisionConfiguration;

btCollisionConfiguration.prototype.__class__ = btCollisionConfiguration;

btCollisionConfiguration.__cache__ = {};

Module["btCollisionConfiguration"] = btCollisionConfiguration;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btCollisionConfiguration.prototype["__destroy__"] = btCollisionConfiguration.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btCollisionConfiguration___destroy___0(self);
};

// Interface: btDbvtBroadphase
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btDbvtBroadphase() {
  this.ptr = _emscripten_bind_btDbvtBroadphase_btDbvtBroadphase_0();
  getCache(btDbvtBroadphase)[this.ptr] = this;
}

btDbvtBroadphase.prototype = Object.create(WrapperObject.prototype);

btDbvtBroadphase.prototype.constructor = btDbvtBroadphase;

btDbvtBroadphase.prototype.__class__ = btDbvtBroadphase;

btDbvtBroadphase.__cache__ = {};

Module["btDbvtBroadphase"] = btDbvtBroadphase;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDbvtBroadphase.prototype["optimize"] = btDbvtBroadphase.prototype.optimize = function() {
  var self = this.ptr;
  _emscripten_bind_btDbvtBroadphase_optimize_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDbvtBroadphase.prototype["__destroy__"] = btDbvtBroadphase.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btDbvtBroadphase___destroy___0(self);
};

// Interface: btRigidBodyConstructionInfo
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btRigidBodyConstructionInfo(mass, motionState, collisionShape, localInertia) {
  if (mass && typeof mass === "object") mass = mass.ptr;
  if (motionState && typeof motionState === "object") motionState = motionState.ptr;
  if (collisionShape && typeof collisionShape === "object") collisionShape = collisionShape.ptr;
  if (localInertia && typeof localInertia === "object") localInertia = localInertia.ptr;
  if (localInertia === undefined) {
    this.ptr = _emscripten_bind_btRigidBodyConstructionInfo_btRigidBodyConstructionInfo_3(mass, motionState, collisionShape);
    getCache(btRigidBodyConstructionInfo)[this.ptr] = this;
    return;
  }
  this.ptr = _emscripten_bind_btRigidBodyConstructionInfo_btRigidBodyConstructionInfo_4(mass, motionState, collisionShape, localInertia);
  getCache(btRigidBodyConstructionInfo)[this.ptr] = this;
}

btRigidBodyConstructionInfo.prototype = Object.create(WrapperObject.prototype);

btRigidBodyConstructionInfo.prototype.constructor = btRigidBodyConstructionInfo;

btRigidBodyConstructionInfo.prototype.__class__ = btRigidBodyConstructionInfo;

btRigidBodyConstructionInfo.__cache__ = {};

Module["btRigidBodyConstructionInfo"] = btRigidBodyConstructionInfo;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBodyConstructionInfo.prototype["__destroy__"] = btRigidBodyConstructionInfo.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btRigidBodyConstructionInfo___destroy___0(self);
};

// Interface: btRigidBody
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btRigidBody(constructionInfo) {
  if (constructionInfo && typeof constructionInfo === "object") constructionInfo = constructionInfo.ptr;
  this.ptr = _emscripten_bind_btRigidBody_btRigidBody_1(constructionInfo);
  getCache(btRigidBody)[this.ptr] = this;
}

btRigidBody.prototype = Object.create(btCollisionObject.prototype);

btRigidBody.prototype.constructor = btRigidBody;

btRigidBody.prototype.__class__ = btRigidBody;

btRigidBody.__cache__ = {};

Module["btRigidBody"] = btRigidBody;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBody.prototype["getMotionState"] = btRigidBody.prototype.getMotionState = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btRigidBody_getMotionState_0(self), btMotionState);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBody.prototype["upcast"] = btRigidBody.prototype.upcast = function(colObj) {
  if (colObj && typeof colObj === "object") colObj = colObj.ptr;
  return wrapPointer(_emscripten_bind_btRigidBody_upcast_1(colObj), btRigidBody);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBody.prototype["getCollisionShape"] = btRigidBody.prototype.getCollisionShape = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btRigidBody_getCollisionShape_0(self), btCollisionShape);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBody.prototype["setActivationState"] = btRigidBody.prototype.setActivationState = function(newState) {
  var self = this.ptr;
  if (newState && typeof newState === "object") newState = newState.ptr;
  _emscripten_bind_btRigidBody_setActivationState_1(self, newState);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBody.prototype["getWorldTransform"] = btRigidBody.prototype.getWorldTransform = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btRigidBody_getWorldTransform_0(self), btTransform);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBody.prototype["setCollisionFlags"] = btRigidBody.prototype.setCollisionFlags = function(flags) {
  var self = this.ptr;
  if (flags && typeof flags === "object") flags = flags.ptr;
  _emscripten_bind_btRigidBody_setCollisionFlags_1(self, flags);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBody.prototype["setWorldTransform"] = btRigidBody.prototype.setWorldTransform = function(worldTrans) {
  var self = this.ptr;
  if (worldTrans && typeof worldTrans === "object") worldTrans = worldTrans.ptr;
  _emscripten_bind_btRigidBody_setWorldTransform_1(self, worldTrans);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBody.prototype["setCollisionShape"] = btRigidBody.prototype.setCollisionShape = function(collisionShape) {
  var self = this.ptr;
  if (collisionShape && typeof collisionShape === "object") collisionShape = collisionShape.ptr;
  _emscripten_bind_btRigidBody_setCollisionShape_1(self, collisionShape);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBody.prototype["getUserIndex"] = btRigidBody.prototype.getUserIndex = function() {
  var self = this.ptr;
  return _emscripten_bind_btRigidBody_getUserIndex_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBody.prototype["setUserIndex"] = btRigidBody.prototype.setUserIndex = function(index) {
  var self = this.ptr;
  if (index && typeof index === "object") index = index.ptr;
  _emscripten_bind_btRigidBody_setUserIndex_1(self, index);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBody.prototype["setUserIndex2"] = btRigidBody.prototype.setUserIndex2 = function(index) {
  var self = this.ptr;
  if (index && typeof index === "object") index = index.ptr;
  _emscripten_bind_btRigidBody_setUserIndex2_1(self, index);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btRigidBody.prototype["__destroy__"] = btRigidBody.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btRigidBody___destroy___0(self);
};

// Interface: btSequentialImpulseConstraintSolver
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btSequentialImpulseConstraintSolver() {
  this.ptr = _emscripten_bind_btSequentialImpulseConstraintSolver_btSequentialImpulseConstraintSolver_0();
  getCache(btSequentialImpulseConstraintSolver)[this.ptr] = this;
}

btSequentialImpulseConstraintSolver.prototype = Object.create(btConstraintSolver.prototype);

btSequentialImpulseConstraintSolver.prototype.constructor = btSequentialImpulseConstraintSolver;

btSequentialImpulseConstraintSolver.prototype.__class__ = btSequentialImpulseConstraintSolver;

btSequentialImpulseConstraintSolver.__cache__ = {};

Module["btSequentialImpulseConstraintSolver"] = btSequentialImpulseConstraintSolver;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btSequentialImpulseConstraintSolver.prototype["__destroy__"] = btSequentialImpulseConstraintSolver.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btSequentialImpulseConstraintSolver___destroy___0(self);
};

// Interface: btDispatcherInfo
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btDispatcherInfo() {
  throw "cannot construct a btDispatcherInfo, no constructor in IDL";
}

btDispatcherInfo.prototype = Object.create(WrapperObject.prototype);

btDispatcherInfo.prototype.constructor = btDispatcherInfo;

btDispatcherInfo.prototype.__class__ = btDispatcherInfo;

btDispatcherInfo.__cache__ = {};

Module["btDispatcherInfo"] = btDispatcherInfo;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDispatcherInfo.prototype["get_m_useContinuous"] = btDispatcherInfo.prototype.get_m_useContinuous = function() {
  var self = this.ptr;
  return !!(_emscripten_bind_btDispatcherInfo_get_m_useContinuous_0(self));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDispatcherInfo.prototype["set_m_useContinuous"] = btDispatcherInfo.prototype.set_m_useContinuous = function(arg0) {
  var self = this.ptr;
  if (arg0 && typeof arg0 === "object") arg0 = arg0.ptr;
  _emscripten_bind_btDispatcherInfo_set_m_useContinuous_1(self, arg0);
};

/** @suppress {checkTypes} */ Object.defineProperty(btDispatcherInfo.prototype, "m_useContinuous", {
  get: btDispatcherInfo.prototype.get_m_useContinuous,
  set: btDispatcherInfo.prototype.set_m_useContinuous
});

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDispatcherInfo.prototype["__destroy__"] = btDispatcherInfo.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btDispatcherInfo___destroy___0(self);
};

// Interface: btDiscreteDynamicsWorld
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btDiscreteDynamicsWorld(dispatcher, pairCache, constraintSolver, collisionConfiguration) {
  if (dispatcher && typeof dispatcher === "object") dispatcher = dispatcher.ptr;
  if (pairCache && typeof pairCache === "object") pairCache = pairCache.ptr;
  if (constraintSolver && typeof constraintSolver === "object") constraintSolver = constraintSolver.ptr;
  if (collisionConfiguration && typeof collisionConfiguration === "object") collisionConfiguration = collisionConfiguration.ptr;
  this.ptr = _emscripten_bind_btDiscreteDynamicsWorld_btDiscreteDynamicsWorld_4(dispatcher, pairCache, constraintSolver, collisionConfiguration);
  getCache(btDiscreteDynamicsWorld)[this.ptr] = this;
}

btDiscreteDynamicsWorld.prototype = Object.create(btDynamicsWorld.prototype);

btDiscreteDynamicsWorld.prototype.constructor = btDiscreteDynamicsWorld;

btDiscreteDynamicsWorld.prototype.__class__ = btDiscreteDynamicsWorld;

btDiscreteDynamicsWorld.__cache__ = {};

Module["btDiscreteDynamicsWorld"] = btDiscreteDynamicsWorld;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["setGravity"] = btDiscreteDynamicsWorld.prototype.setGravity = function(gravity) {
  var self = this.ptr;
  if (gravity && typeof gravity === "object") gravity = gravity.ptr;
  _emscripten_bind_btDiscreteDynamicsWorld_setGravity_1(self, gravity);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["addRigidBody"] = btDiscreteDynamicsWorld.prototype.addRigidBody = function(body, group, mask) {
  var self = this.ptr;
  if (body && typeof body === "object") body = body.ptr;
  if (group && typeof group === "object") group = group.ptr;
  if (mask && typeof mask === "object") mask = mask.ptr;
  if (group === undefined) {
    _emscripten_bind_btDiscreteDynamicsWorld_addRigidBody_1(self, body);
    return;
  }
  if (mask === undefined) {
    _emscripten_bind_btDiscreteDynamicsWorld_addRigidBody_2(self, body, group);
    return;
  }
  _emscripten_bind_btDiscreteDynamicsWorld_addRigidBody_3(self, body, group, mask);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["removeRigidBody"] = btDiscreteDynamicsWorld.prototype.removeRigidBody = function(body) {
  var self = this.ptr;
  if (body && typeof body === "object") body = body.ptr;
  _emscripten_bind_btDiscreteDynamicsWorld_removeRigidBody_1(self, body);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["stepSimulation"] = btDiscreteDynamicsWorld.prototype.stepSimulation = function(timeStep, maxSubSteps, fixedTimeStep) {
  var self = this.ptr;
  if (timeStep && typeof timeStep === "object") timeStep = timeStep.ptr;
  if (maxSubSteps && typeof maxSubSteps === "object") maxSubSteps = maxSubSteps.ptr;
  if (fixedTimeStep && typeof fixedTimeStep === "object") fixedTimeStep = fixedTimeStep.ptr;
  if (maxSubSteps === undefined) {
    return _emscripten_bind_btDiscreteDynamicsWorld_stepSimulation_1(self, timeStep);
  }
  if (fixedTimeStep === undefined) {
    return _emscripten_bind_btDiscreteDynamicsWorld_stepSimulation_2(self, timeStep, maxSubSteps);
  }
  return _emscripten_bind_btDiscreteDynamicsWorld_stepSimulation_3(self, timeStep, maxSubSteps, fixedTimeStep);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["substepSimulation"] = btDiscreteDynamicsWorld.prototype.substepSimulation = function(fixedTimeStep) {
  var self = this.ptr;
  if (fixedTimeStep && typeof fixedTimeStep === "object") fixedTimeStep = fixedTimeStep.ptr;
  _emscripten_bind_btDiscreteDynamicsWorld_substepSimulation_1(self, fixedTimeStep);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["getDispatcher"] = btDiscreteDynamicsWorld.prototype.getDispatcher = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btDiscreteDynamicsWorld_getDispatcher_0(self), btDispatcher);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["getDispatchInfo"] = btDiscreteDynamicsWorld.prototype.getDispatchInfo = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btDiscreteDynamicsWorld_getDispatchInfo_0(self), btDispatcherInfo);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["addCollisionObject"] = btDiscreteDynamicsWorld.prototype.addCollisionObject = function(collisionObject, collisionFilterGroup, collisionFilterMask) {
  var self = this.ptr;
  if (collisionObject && typeof collisionObject === "object") collisionObject = collisionObject.ptr;
  if (collisionFilterGroup && typeof collisionFilterGroup === "object") collisionFilterGroup = collisionFilterGroup.ptr;
  if (collisionFilterMask && typeof collisionFilterMask === "object") collisionFilterMask = collisionFilterMask.ptr;
  if (collisionFilterGroup === undefined) {
    _emscripten_bind_btDiscreteDynamicsWorld_addCollisionObject_1(self, collisionObject);
    return;
  }
  if (collisionFilterMask === undefined) {
    _emscripten_bind_btDiscreteDynamicsWorld_addCollisionObject_2(self, collisionObject, collisionFilterGroup);
    return;
  }
  _emscripten_bind_btDiscreteDynamicsWorld_addCollisionObject_3(self, collisionObject, collisionFilterGroup, collisionFilterMask);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["removeCollisionObject"] = btDiscreteDynamicsWorld.prototype.removeCollisionObject = function(collisionObject) {
  var self = this.ptr;
  if (collisionObject && typeof collisionObject === "object") collisionObject = collisionObject.ptr;
  _emscripten_bind_btDiscreteDynamicsWorld_removeCollisionObject_1(self, collisionObject);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["getBroadphase"] = btDiscreteDynamicsWorld.prototype.getBroadphase = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btDiscreteDynamicsWorld_getBroadphase_0(self), btBroadphaseInterface);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["contactPairTestBinary"] = btDiscreteDynamicsWorld.prototype.contactPairTestBinary = function(colObjA, colObjB, minPenetrationDepth) {
  var self = this.ptr;
  if (colObjA && typeof colObjA === "object") colObjA = colObjA.ptr;
  if (colObjB && typeof colObjB === "object") colObjB = colObjB.ptr;
  if (minPenetrationDepth && typeof minPenetrationDepth === "object") minPenetrationDepth = minPenetrationDepth.ptr;
  return !!(_emscripten_bind_btDiscreteDynamicsWorld_contactPairTestBinary_3(self, colObjA, colObjB, minPenetrationDepth));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["addAction"] = btDiscreteDynamicsWorld.prototype.addAction = function(action) {
  var self = this.ptr;
  if (action && typeof action === "object") action = action.ptr;
  _emscripten_bind_btDiscreteDynamicsWorld_addAction_1(self, action);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDiscreteDynamicsWorld.prototype["__destroy__"] = btDiscreteDynamicsWorld.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btDiscreteDynamicsWorld___destroy___0(self);
};

// Interface: btJumpPad
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btJumpPad(ghostObject, zoneId, baseImpulse, speedScaling, cooldownSeconds) {
  if (ghostObject && typeof ghostObject === "object") ghostObject = ghostObject.ptr;
  if (zoneId && typeof zoneId === "object") zoneId = zoneId.ptr;
  if (baseImpulse && typeof baseImpulse === "object") baseImpulse = baseImpulse.ptr;
  if (speedScaling && typeof speedScaling === "object") speedScaling = speedScaling.ptr;
  if (cooldownSeconds && typeof cooldownSeconds === "object") cooldownSeconds = cooldownSeconds.ptr;
  this.ptr = _emscripten_bind_btJumpPad_btJumpPad_5(ghostObject, zoneId, baseImpulse, speedScaling, cooldownSeconds);
  getCache(btJumpPad)[this.ptr] = this;
}

btJumpPad.prototype = Object.create(WrapperObject.prototype);

btJumpPad.prototype.constructor = btJumpPad;

btJumpPad.prototype.__class__ = btJumpPad;

btJumpPad.__cache__ = {};

Module["btJumpPad"] = btJumpPad;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btJumpPad.prototype["setDirection"] = btJumpPad.prototype.setDirection = function(direction) {
  var self = this.ptr;
  if (direction && typeof direction === "object") direction = direction.ptr;
  _emscripten_bind_btJumpPad_setDirection_1(self, direction);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btJumpPad.prototype["setBaseImpulse"] = btJumpPad.prototype.setBaseImpulse = function(impulse) {
  var self = this.ptr;
  if (impulse && typeof impulse === "object") impulse = impulse.ptr;
  _emscripten_bind_btJumpPad_setBaseImpulse_1(self, impulse);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btJumpPad.prototype["setSpeedScaling"] = btJumpPad.prototype.setSpeedScaling = function(scaling) {
  var self = this.ptr;
  if (scaling && typeof scaling === "object") scaling = scaling.ptr;
  _emscripten_bind_btJumpPad_setSpeedScaling_1(self, scaling);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btJumpPad.prototype["setCooldownSeconds"] = btJumpPad.prototype.setCooldownSeconds = function(seconds) {
  var self = this.ptr;
  if (seconds && typeof seconds === "object") seconds = seconds.ptr;
  _emscripten_bind_btJumpPad_setCooldownSeconds_1(self, seconds);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btJumpPad.prototype["getZoneId"] = btJumpPad.prototype.getZoneId = function() {
  var self = this.ptr;
  return _emscripten_bind_btJumpPad_getZoneId_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btJumpPad.prototype["setEnabled"] = btJumpPad.prototype.setEnabled = function(enabled) {
  var self = this.ptr;
  if (enabled && typeof enabled === "object") enabled = enabled.ptr;
  _emscripten_bind_btJumpPad_setEnabled_1(self, enabled);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btJumpPad.prototype["setUseExternalVelocity"] = btJumpPad.prototype.setUseExternalVelocity = function(useExternal) {
  var self = this.ptr;
  if (useExternal && typeof useExternal === "object") useExternal = useExternal.ptr;
  _emscripten_bind_btJumpPad_setUseExternalVelocity_1(self, useExternal);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btJumpPad.prototype["__destroy__"] = btJumpPad.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btJumpPad___destroy___0(self);
};

// Interface: btBoostZone
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btBoostZone(ghostObject, zoneId, strength, directionalBias) {
  if (ghostObject && typeof ghostObject === "object") ghostObject = ghostObject.ptr;
  if (zoneId && typeof zoneId === "object") zoneId = zoneId.ptr;
  if (strength && typeof strength === "object") strength = strength.ptr;
  if (directionalBias && typeof directionalBias === "object") directionalBias = directionalBias.ptr;
  this.ptr = _emscripten_bind_btBoostZone_btBoostZone_4(ghostObject, zoneId, strength, directionalBias);
  getCache(btBoostZone)[this.ptr] = this;
}

btBoostZone.prototype = Object.create(WrapperObject.prototype);

btBoostZone.prototype.constructor = btBoostZone;

btBoostZone.prototype.__class__ = btBoostZone;

btBoostZone.__cache__ = {};

Module["btBoostZone"] = btBoostZone;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBoostZone.prototype["setDirection"] = btBoostZone.prototype.setDirection = function(direction) {
  var self = this.ptr;
  if (direction && typeof direction === "object") direction = direction.ptr;
  _emscripten_bind_btBoostZone_setDirection_1(self, direction);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBoostZone.prototype["setStrength"] = btBoostZone.prototype.setStrength = function(strength) {
  var self = this.ptr;
  if (strength && typeof strength === "object") strength = strength.ptr;
  _emscripten_bind_btBoostZone_setStrength_1(self, strength);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBoostZone.prototype["setDirectionalBias"] = btBoostZone.prototype.setDirectionalBias = function(bias) {
  var self = this.ptr;
  if (bias && typeof bias === "object") bias = bias.ptr;
  _emscripten_bind_btBoostZone_setDirectionalBias_1(self, bias);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBoostZone.prototype["getZoneId"] = btBoostZone.prototype.getZoneId = function() {
  var self = this.ptr;
  return _emscripten_bind_btBoostZone_getZoneId_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBoostZone.prototype["setEnabled"] = btBoostZone.prototype.setEnabled = function(enabled) {
  var self = this.ptr;
  if (enabled && typeof enabled === "object") enabled = enabled.ptr;
  _emscripten_bind_btBoostZone_setEnabled_1(self, enabled);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btBoostZone.prototype["__destroy__"] = btBoostZone.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btBoostZone___destroy___0(self);
};

// Interface: btSensor
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btSensor(ghostObject, zoneId, minPenetrationDepth) {
  if (ghostObject && typeof ghostObject === "object") ghostObject = ghostObject.ptr;
  if (zoneId && typeof zoneId === "object") zoneId = zoneId.ptr;
  if (minPenetrationDepth && typeof minPenetrationDepth === "object") minPenetrationDepth = minPenetrationDepth.ptr;
  this.ptr = _emscripten_bind_btSensor_btSensor_3(ghostObject, zoneId, minPenetrationDepth);
  getCache(btSensor)[this.ptr] = this;
}

btSensor.prototype = Object.create(WrapperObject.prototype);

btSensor.prototype.constructor = btSensor;

btSensor.prototype.__class__ = btSensor;

btSensor.__cache__ = {};

Module["btSensor"] = btSensor;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btSensor.prototype["getZoneId"] = btSensor.prototype.getZoneId = function() {
  var self = this.ptr;
  return _emscripten_bind_btSensor_getZoneId_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btSensor.prototype["isOverlapping"] = btSensor.prototype.isOverlapping = function() {
  var self = this.ptr;
  return !!(_emscripten_bind_btSensor_isOverlapping_0(self));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btSensor.prototype["setEnabled"] = btSensor.prototype.setEnabled = function(enabled) {
  var self = this.ptr;
  if (enabled && typeof enabled === "object") enabled = enabled.ptr;
  _emscripten_bind_btSensor_setEnabled_1(self, enabled);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btSensor.prototype["__destroy__"] = btSensor.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btSensor___destroy___0(self);
};

// Interface: btDashToken
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btDashToken(ghostObject, zoneId, chargesGranted, minPenetrationDepth) {
  if (ghostObject && typeof ghostObject === "object") ghostObject = ghostObject.ptr;
  if (zoneId && typeof zoneId === "object") zoneId = zoneId.ptr;
  if (chargesGranted && typeof chargesGranted === "object") chargesGranted = chargesGranted.ptr;
  if (minPenetrationDepth && typeof minPenetrationDepth === "object") minPenetrationDepth = minPenetrationDepth.ptr;
  this.ptr = _emscripten_bind_btDashToken_btDashToken_4(ghostObject, zoneId, chargesGranted, minPenetrationDepth);
  getCache(btDashToken)[this.ptr] = this;
}

btDashToken.prototype = Object.create(WrapperObject.prototype);

btDashToken.prototype.constructor = btDashToken;

btDashToken.prototype.__class__ = btDashToken;

btDashToken.__cache__ = {};

Module["btDashToken"] = btDashToken;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDashToken.prototype["getZoneId"] = btDashToken.prototype.getZoneId = function() {
  var self = this.ptr;
  return _emscripten_bind_btDashToken_getZoneId_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDashToken.prototype["setEnabled"] = btDashToken.prototype.setEnabled = function(enabled) {
  var self = this.ptr;
  if (enabled && typeof enabled === "object") enabled = enabled.ptr;
  _emscripten_bind_btDashToken_setEnabled_1(self, enabled);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDashToken.prototype["setChargesGranted"] = btDashToken.prototype.setChargesGranted = function(chargesGranted) {
  var self = this.ptr;
  if (chargesGranted && typeof chargesGranted === "object") chargesGranted = chargesGranted.ptr;
  _emscripten_bind_btDashToken_setChargesGranted_1(self, chargesGranted);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDashToken.prototype["getChargesGranted"] = btDashToken.prototype.getChargesGranted = function() {
  var self = this.ptr;
  return _emscripten_bind_btDashToken_getChargesGranted_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDashToken.prototype["setActive"] = btDashToken.prototype.setActive = function(active) {
  var self = this.ptr;
  if (active && typeof active === "object") active = active.ptr;
  _emscripten_bind_btDashToken_setActive_1(self, active);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDashToken.prototype["isActive"] = btDashToken.prototype.isActive = function() {
  var self = this.ptr;
  return !!(_emscripten_bind_btDashToken_isActive_0(self));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btDashToken.prototype["__destroy__"] = btDashToken.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btDashToken___destroy___0(self);
};

// Interface: btKinematicCharacterController
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btKinematicCharacterController(ghostObject, convexShape, stepHeight, up) {
  if (ghostObject && typeof ghostObject === "object") ghostObject = ghostObject.ptr;
  if (convexShape && typeof convexShape === "object") convexShape = convexShape.ptr;
  if (stepHeight && typeof stepHeight === "object") stepHeight = stepHeight.ptr;
  if (up && typeof up === "object") up = up.ptr;
  if (up === undefined) {
    this.ptr = _emscripten_bind_btKinematicCharacterController_btKinematicCharacterController_3(ghostObject, convexShape, stepHeight);
    getCache(btKinematicCharacterController)[this.ptr] = this;
    return;
  }
  this.ptr = _emscripten_bind_btKinematicCharacterController_btKinematicCharacterController_4(ghostObject, convexShape, stepHeight, up);
  getCache(btKinematicCharacterController)[this.ptr] = this;
}

btKinematicCharacterController.prototype = Object.create(btActionInterface.prototype);

btKinematicCharacterController.prototype.constructor = btKinematicCharacterController;

btKinematicCharacterController.prototype.__class__ = btKinematicCharacterController;

btKinematicCharacterController.__cache__ = {};

Module["btKinematicCharacterController"] = btKinematicCharacterController;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setInputState"] = btKinematicCharacterController.prototype.setInputState = function(keyFlags, theta, phi, movementEnabled) {
  var self = this.ptr;
  if (keyFlags && typeof keyFlags === "object") keyFlags = keyFlags.ptr;
  if (theta && typeof theta === "object") theta = theta.ptr;
  if (phi && typeof phi === "object") phi = phi.ptr;
  if (movementEnabled && typeof movementEnabled === "object") movementEnabled = movementEnabled.ptr;
  _emscripten_bind_btKinematicCharacterController_setInputState_4(self, keyFlags, theta, phi, movementEnabled);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setMoveSpeed"] = btKinematicCharacterController.prototype.setMoveSpeed = function(ground, air) {
  var self = this.ptr;
  if (ground && typeof ground === "object") ground = ground.ptr;
  if (air && typeof air === "object") air = air.ptr;
  _emscripten_bind_btKinematicCharacterController_setMoveSpeed_2(self, ground, air);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["isBoostEffective"] = btKinematicCharacterController.prototype.isBoostEffective = function() {
  var self = this.ptr;
  return !!(_emscripten_bind_btKinematicCharacterController_isBoostEffective_0(self));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getBoostChargeRatio"] = btKinematicCharacterController.prototype.getBoostChargeRatio = function() {
  var self = this.ptr;
  return _emscripten_bind_btKinematicCharacterController_getBoostChargeRatio_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["defineSurfaceMaterial"] = btKinematicCharacterController.prototype.defineSurfaceMaterial = function(materialId) {
  var self = this.ptr;
  if (materialId && typeof materialId === "object") materialId = materialId.ptr;
  _emscripten_bind_btKinematicCharacterController_defineSurfaceMaterial_1(self, materialId);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setSurfaceMaterialBoost"] = btKinematicCharacterController.prototype.setSurfaceMaterialBoost = function(materialId, targetSpeed, jumpRetention, rampSeconds, followSlope) {
  var self = this.ptr;
  if (materialId && typeof materialId === "object") materialId = materialId.ptr;
  if (targetSpeed && typeof targetSpeed === "object") targetSpeed = targetSpeed.ptr;
  if (jumpRetention && typeof jumpRetention === "object") jumpRetention = jumpRetention.ptr;
  if (rampSeconds && typeof rampSeconds === "object") rampSeconds = rampSeconds.ptr;
  if (followSlope && typeof followSlope === "object") followSlope = followSlope.ptr;
  return !!(_emscripten_bind_btKinematicCharacterController_setSurfaceMaterialBoost_5(self, materialId, targetSpeed, jumpRetention, rampSeconds, followSlope));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setSurfaceMaterialClimb"] = btKinematicCharacterController.prototype.setSurfaceMaterialClimb = function(materialId, maxClimbAngle, slideMinAngle, slideMaxSpeed) {
  var self = this.ptr;
  if (materialId && typeof materialId === "object") materialId = materialId.ptr;
  if (maxClimbAngle && typeof maxClimbAngle === "object") maxClimbAngle = maxClimbAngle.ptr;
  if (slideMinAngle && typeof slideMinAngle === "object") slideMinAngle = slideMinAngle.ptr;
  if (slideMaxSpeed && typeof slideMaxSpeed === "object") slideMaxSpeed = slideMaxSpeed.ptr;
  return !!(_emscripten_bind_btKinematicCharacterController_setSurfaceMaterialClimb_4(self, materialId, maxClimbAngle, slideMinAngle, slideMaxSpeed));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setSurfaceMaterialExtVelGroundDamping"] = btKinematicCharacterController.prototype.setSurfaceMaterialExtVelGroundDamping = function(materialId, x, y, z) {
  var self = this.ptr;
  if (materialId && typeof materialId === "object") materialId = materialId.ptr;
  if (x && typeof x === "object") x = x.ptr;
  if (y && typeof y === "object") y = y.ptr;
  if (z && typeof z === "object") z = z.ptr;
  return !!(_emscripten_bind_btKinematicCharacterController_setSurfaceMaterialExtVelGroundDamping_4(self, materialId, x, y, z));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["assignSurfaceMaterial"] = btKinematicCharacterController.prototype.assignSurfaceMaterial = function(obj, materialId) {
  var self = this.ptr;
  if (obj && typeof obj === "object") obj = obj.ptr;
  if (materialId && typeof materialId === "object") materialId = materialId.ptr;
  return !!(_emscripten_bind_btKinematicCharacterController_assignSurfaceMaterial_2(self, obj, materialId));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getFloorSurfaceMaterialId"] = btKinematicCharacterController.prototype.getFloorSurfaceMaterialId = function() {
  var self = this.ptr;
  return _emscripten_bind_btKinematicCharacterController_getFloorSurfaceMaterialId_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setTopDownMode"] = btKinematicCharacterController.prototype.setTopDownMode = function(topDown) {
  var self = this.ptr;
  if (topDown && typeof topDown === "object") topDown = topDown.ptr;
  _emscripten_bind_btKinematicCharacterController_setTopDownMode_1(self, topDown);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setMinJumpDelay"] = btKinematicCharacterController.prototype.setMinJumpDelay = function(seconds) {
  var self = this.ptr;
  if (seconds && typeof seconds === "object") seconds = seconds.ptr;
  _emscripten_bind_btKinematicCharacterController_setMinJumpDelay_1(self, seconds);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setCoyoteTime"] = btKinematicCharacterController.prototype.setCoyoteTime = function(seconds) {
  var self = this.ptr;
  if (seconds && typeof seconds === "object") seconds = seconds.ptr;
  _emscripten_bind_btKinematicCharacterController_setCoyoteTime_1(self, seconds);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setBoostArmLeniency"] = btKinematicCharacterController.prototype.setBoostArmLeniency = function(seconds) {
  var self = this.ptr;
  if (seconds && typeof seconds === "object") seconds = seconds.ptr;
  _emscripten_bind_btKinematicCharacterController_setBoostArmLeniency_1(self, seconds);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setDashConfig"] = btKinematicCharacterController.prototype.setDashConfig = function(enabled, magnitude, minDelay, useExternalVelocity) {
  var self = this.ptr;
  if (enabled && typeof enabled === "object") enabled = enabled.ptr;
  if (magnitude && typeof magnitude === "object") magnitude = magnitude.ptr;
  if (minDelay && typeof minDelay === "object") minDelay = minDelay.ptr;
  if (useExternalVelocity && typeof useExternalVelocity === "object") useExternalVelocity = useExternalVelocity.ptr;
  _emscripten_bind_btKinematicCharacterController_setDashConfig_4(self, enabled, magnitude, minDelay, useExternalVelocity);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setDashDirectionMode"] = btKinematicCharacterController.prototype.setDashDirectionMode = function(mode) {
  var self = this.ptr;
  if (mode && typeof mode === "object") mode = mode.ptr;
  _emscripten_bind_btKinematicCharacterController_setDashDirectionMode_1(self, mode);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setDashVerticalUseJump"] = btKinematicCharacterController.prototype.setDashVerticalUseJump = function(useJump) {
  var self = this.ptr;
  if (useJump && typeof useJump === "object") useJump = useJump.ptr;
  _emscripten_bind_btKinematicCharacterController_setDashVerticalUseJump_1(self, useJump);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setDashCancelFallVelocity"] = btKinematicCharacterController.prototype.setDashCancelFallVelocity = function(cancel) {
  var self = this.ptr;
  if (cancel && typeof cancel === "object") cancel = cancel.ptr;
  _emscripten_bind_btKinematicCharacterController_setDashCancelFallVelocity_1(self, cancel);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setDashCharges"] = btKinematicCharacterController.prototype.setDashCharges = function(charges) {
  var self = this.ptr;
  if (charges && typeof charges === "object") charges = charges.ptr;
  _emscripten_bind_btKinematicCharacterController_setDashCharges_1(self, charges);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getLastJumpTime"] = btKinematicCharacterController.prototype.getLastJumpTime = function() {
  var self = this.ptr;
  return _emscripten_bind_btKinematicCharacterController_getLastJumpTime_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getLastDashTime"] = btKinematicCharacterController.prototype.getLastDashTime = function() {
  var self = this.ptr;
  return _emscripten_bind_btKinematicCharacterController_getLastDashTime_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getDashCharges"] = btKinematicCharacterController.prototype.getDashCharges = function() {
  var self = this.ptr;
  return _emscripten_bind_btKinematicCharacterController_getDashCharges_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getLastMoveDir"] = btKinematicCharacterController.prototype.getLastMoveDir = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btKinematicCharacterController_getLastMoveDir_0(self), btVector3);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getLastDashDir"] = btKinematicCharacterController.prototype.getLastDashDir = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btKinematicCharacterController_getLastDashDir_0(self), btVector3);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getWalkDirection"] = btKinematicCharacterController.prototype.getWalkDirection = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btKinematicCharacterController_getWalkDirection_0(self), btVector3);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["warp"] = btKinematicCharacterController.prototype.warp = function(origin) {
  var self = this.ptr;
  if (origin && typeof origin === "object") origin = origin.ptr;
  _emscripten_bind_btKinematicCharacterController_warp_1(self, origin);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getPosition"] = btKinematicCharacterController.prototype.getPosition = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btKinematicCharacterController_getPosition_0(self), btVector3);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setFallSpeed"] = btKinematicCharacterController.prototype.setFallSpeed = function(fallSpeed) {
  var self = this.ptr;
  if (fallSpeed && typeof fallSpeed === "object") fallSpeed = fallSpeed.ptr;
  _emscripten_bind_btKinematicCharacterController_setFallSpeed_1(self, fallSpeed);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setJumpSpeed"] = btKinematicCharacterController.prototype.setJumpSpeed = function(jumpSpeed) {
  var self = this.ptr;
  if (jumpSpeed && typeof jumpSpeed === "object") jumpSpeed = jumpSpeed.ptr;
  _emscripten_bind_btKinematicCharacterController_setJumpSpeed_1(self, jumpSpeed);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setGravity"] = btKinematicCharacterController.prototype.setGravity = function(gravity) {
  var self = this.ptr;
  if (gravity && typeof gravity === "object") gravity = gravity.ptr;
  _emscripten_bind_btKinematicCharacterController_setGravity_1(self, gravity);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setMaxSlope"] = btKinematicCharacterController.prototype.setMaxSlope = function(slopeRadians) {
  var self = this.ptr;
  if (slopeRadians && typeof slopeRadians === "object") slopeRadians = slopeRadians.ptr;
  _emscripten_bind_btKinematicCharacterController_setMaxSlope_1(self, slopeRadians);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setMaxPenetrationDepth"] = btKinematicCharacterController.prototype.setMaxPenetrationDepth = function(d) {
  var self = this.ptr;
  if (d && typeof d === "object") d = d.ptr;
  _emscripten_bind_btKinematicCharacterController_setMaxPenetrationDepth_1(self, d);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setStepHeight"] = btKinematicCharacterController.prototype.setStepHeight = function(h) {
  var self = this.ptr;
  if (h && typeof h === "object") h = h.ptr;
  _emscripten_bind_btKinematicCharacterController_setStepHeight_1(self, h);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setExternalVelocity"] = btKinematicCharacterController.prototype.setExternalVelocity = function(velocity) {
  var self = this.ptr;
  if (velocity && typeof velocity === "object") velocity = velocity.ptr;
  _emscripten_bind_btKinematicCharacterController_setExternalVelocity_1(self, velocity);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setExternalVelocityAirDampingFactor"] = btKinematicCharacterController.prototype.setExternalVelocityAirDampingFactor = function(damping) {
  var self = this.ptr;
  if (damping && typeof damping === "object") damping = damping.ptr;
  _emscripten_bind_btKinematicCharacterController_setExternalVelocityAirDampingFactor_1(self, damping);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setExternalVelocityAirIdleDampingFactor"] = btKinematicCharacterController.prototype.setExternalVelocityAirIdleDampingFactor = function(damping) {
  var self = this.ptr;
  if (damping && typeof damping === "object") damping = damping.ptr;
  _emscripten_bind_btKinematicCharacterController_setExternalVelocityAirIdleDampingFactor_1(self, damping);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setExternalVelocityGroundDampingFactor"] = btKinematicCharacterController.prototype.setExternalVelocityGroundDampingFactor = function(damping) {
  var self = this.ptr;
  if (damping && typeof damping === "object") damping = damping.ptr;
  _emscripten_bind_btKinematicCharacterController_setExternalVelocityGroundDampingFactor_1(self, damping);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setSlopeSlide"] = btKinematicCharacterController.prototype.setSlopeSlide = function(minAngle, maxSpeed) {
  var self = this.ptr;
  if (minAngle && typeof minAngle === "object") minAngle = minAngle.ptr;
  if (maxSpeed && typeof maxSpeed === "object") maxSpeed = maxSpeed.ptr;
  _emscripten_bind_btKinematicCharacterController_setSlopeSlide_2(self, minAngle, maxSpeed);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setOnGround"] = btKinematicCharacterController.prototype.setOnGround = function(onGround) {
  var self = this.ptr;
  if (onGround && typeof onGround === "object") onGround = onGround.ptr;
  _emscripten_bind_btKinematicCharacterController_setOnGround_1(self, onGround);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setVerticalVelocity"] = btKinematicCharacterController.prototype.setVerticalVelocity = function(vel) {
  var self = this.ptr;
  if (vel && typeof vel === "object") vel = vel.ptr;
  _emscripten_bind_btKinematicCharacterController_setVerticalVelocity_1(self, vel);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setGravityShapeRiseMultiplier"] = btKinematicCharacterController.prototype.setGravityShapeRiseMultiplier = function(v) {
  var self = this.ptr;
  if (v && typeof v === "object") v = v.ptr;
  _emscripten_bind_btKinematicCharacterController_setGravityShapeRiseMultiplier_1(self, v);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setGravityShapeApexMultiplier"] = btKinematicCharacterController.prototype.setGravityShapeApexMultiplier = function(v) {
  var self = this.ptr;
  if (v && typeof v === "object") v = v.ptr;
  _emscripten_bind_btKinematicCharacterController_setGravityShapeApexMultiplier_1(self, v);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setGravityShapeFallMultiplier"] = btKinematicCharacterController.prototype.setGravityShapeFallMultiplier = function(v) {
  var self = this.ptr;
  if (v && typeof v === "object") v = v.ptr;
  _emscripten_bind_btKinematicCharacterController_setGravityShapeFallMultiplier_1(self, v);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setGravityShapeApexThreshold"] = btKinematicCharacterController.prototype.setGravityShapeApexThreshold = function(v) {
  var self = this.ptr;
  if (v && typeof v === "object") v = v.ptr;
  _emscripten_bind_btKinematicCharacterController_setGravityShapeApexThreshold_1(self, v);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setGravityShapeKneeWidth"] = btKinematicCharacterController.prototype.setGravityShapeKneeWidth = function(v) {
  var self = this.ptr;
  if (v && typeof v === "object") v = v.ptr;
  _emscripten_bind_btKinematicCharacterController_setGravityShapeKneeWidth_1(self, v);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["setGravityShapeOnlyJumps"] = btKinematicCharacterController.prototype.setGravityShapeOnlyJumps = function(v) {
  var self = this.ptr;
  if (v && typeof v === "object") v = v.ptr;
  _emscripten_bind_btKinematicCharacterController_setGravityShapeOnlyJumps_1(self, v);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["onGround"] = btKinematicCharacterController.prototype.onGround = function() {
  var self = this.ptr;
  return !!(_emscripten_bind_btKinematicCharacterController_onGround_0(self));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getVerticalVelocity"] = btKinematicCharacterController.prototype.getVerticalVelocity = function() {
  var self = this.ptr;
  return _emscripten_bind_btKinematicCharacterController_getVerticalVelocity_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getJumpAxis"] = btKinematicCharacterController.prototype.getJumpAxis = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btKinematicCharacterController_getJumpAxis_0(self), btVector3);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getExternalVelocity"] = btKinematicCharacterController.prototype.getExternalVelocity = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btKinematicCharacterController_getExternalVelocity_0(self), btVector3);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["isJumping"] = btKinematicCharacterController.prototype.isJumping = function() {
  var self = this.ptr;
  return !!(_emscripten_bind_btKinematicCharacterController_isJumping_0(self));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getFloorUserIndex"] = btKinematicCharacterController.prototype.getFloorUserIndex = function() {
  var self = this.ptr;
  return _emscripten_bind_btKinematicCharacterController_getFloorUserIndex_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["addJumpPad"] = btKinematicCharacterController.prototype.addJumpPad = function(pad) {
  var self = this.ptr;
  if (pad && typeof pad === "object") pad = pad.ptr;
  _emscripten_bind_btKinematicCharacterController_addJumpPad_1(self, pad);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["removeJumpPad"] = btKinematicCharacterController.prototype.removeJumpPad = function(pad) {
  var self = this.ptr;
  if (pad && typeof pad === "object") pad = pad.ptr;
  _emscripten_bind_btKinematicCharacterController_removeJumpPad_1(self, pad);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["addBoostZone"] = btKinematicCharacterController.prototype.addBoostZone = function(zone) {
  var self = this.ptr;
  if (zone && typeof zone === "object") zone = zone.ptr;
  _emscripten_bind_btKinematicCharacterController_addBoostZone_1(self, zone);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["removeBoostZone"] = btKinematicCharacterController.prototype.removeBoostZone = function(zone) {
  var self = this.ptr;
  if (zone && typeof zone === "object") zone = zone.ptr;
  _emscripten_bind_btKinematicCharacterController_removeBoostZone_1(self, zone);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["addSensor"] = btKinematicCharacterController.prototype.addSensor = function(sensor) {
  var self = this.ptr;
  if (sensor && typeof sensor === "object") sensor = sensor.ptr;
  _emscripten_bind_btKinematicCharacterController_addSensor_1(self, sensor);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["removeSensor"] = btKinematicCharacterController.prototype.removeSensor = function(sensor) {
  var self = this.ptr;
  if (sensor && typeof sensor === "object") sensor = sensor.ptr;
  _emscripten_bind_btKinematicCharacterController_removeSensor_1(self, sensor);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["addDashToken"] = btKinematicCharacterController.prototype.addDashToken = function(token) {
  var self = this.ptr;
  if (token && typeof token === "object") token = token.ptr;
  _emscripten_bind_btKinematicCharacterController_addDashToken_1(self, token);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["removeDashToken"] = btKinematicCharacterController.prototype.removeDashToken = function(token) {
  var self = this.ptr;
  if (token && typeof token === "object") token = token.ptr;
  _emscripten_bind_btKinematicCharacterController_removeDashToken_1(self, token);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["captureInitialDashState"] = btKinematicCharacterController.prototype.captureInitialDashState = function() {
  var self = this.ptr;
  _emscripten_bind_btKinematicCharacterController_captureInitialDashState_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["saveDashCheckpointState"] = btKinematicCharacterController.prototype.saveDashCheckpointState = function() {
  var self = this.ptr;
  _emscripten_bind_btKinematicCharacterController_saveDashCheckpointState_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["restoreDashCheckpointState"] = btKinematicCharacterController.prototype.restoreDashCheckpointState = function() {
  var self = this.ptr;
  _emscripten_bind_btKinematicCharacterController_restoreDashCheckpointState_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["resetDashStateForNewRun"] = btKinematicCharacterController.prototype.resetDashStateForNewRun = function() {
  var self = this.ptr;
  _emscripten_bind_btKinematicCharacterController_resetDashStateForNewRun_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getNumPendingEvents"] = btKinematicCharacterController.prototype.getNumPendingEvents = function() {
  var self = this.ptr;
  return _emscripten_bind_btKinematicCharacterController_getNumPendingEvents_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getPendingEventId"] = btKinematicCharacterController.prototype.getPendingEventId = function(index) {
  var self = this.ptr;
  if (index && typeof index === "object") index = index.ptr;
  return _emscripten_bind_btKinematicCharacterController_getPendingEventId_1(self, index);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getPendingEventType"] = btKinematicCharacterController.prototype.getPendingEventType = function(index) {
  var self = this.ptr;
  if (index && typeof index === "object") index = index.ptr;
  return _emscripten_bind_btKinematicCharacterController_getPendingEventType_1(self, index);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["clearPendingEvents"] = btKinematicCharacterController.prototype.clearPendingEvents = function() {
  var self = this.ptr;
  _emscripten_bind_btKinematicCharacterController_clearPendingEvents_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["cameraRayTest"] = btKinematicCharacterController.prototype.cameraRayTest = function(world, fromX, fromY, fromZ, toX, toY, toZ) {
  var self = this.ptr;
  if (world && typeof world === "object") world = world.ptr;
  if (fromX && typeof fromX === "object") fromX = fromX.ptr;
  if (fromY && typeof fromY === "object") fromY = fromY.ptr;
  if (fromZ && typeof fromZ === "object") fromZ = fromZ.ptr;
  if (toX && typeof toX === "object") toX = toX.ptr;
  if (toY && typeof toY === "object") toY = toY.ptr;
  if (toZ && typeof toZ === "object") toZ = toZ.ptr;
  return _emscripten_bind_btKinematicCharacterController_cameraRayTest_7(self, world, fromX, fromY, fromZ, toX, toY, toZ);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["cameraSphereSweep"] = btKinematicCharacterController.prototype.cameraSphereSweep = function(world, fromX, fromY, fromZ, toX, toY, toZ, radius, nonPermeableOnly) {
  var self = this.ptr;
  if (world && typeof world === "object") world = world.ptr;
  if (fromX && typeof fromX === "object") fromX = fromX.ptr;
  if (fromY && typeof fromY === "object") fromY = fromY.ptr;
  if (fromZ && typeof fromZ === "object") fromZ = fromZ.ptr;
  if (toX && typeof toX === "object") toX = toX.ptr;
  if (toY && typeof toY === "object") toY = toY.ptr;
  if (toZ && typeof toZ === "object") toZ = toZ.ptr;
  if (radius && typeof radius === "object") radius = radius.ptr;
  if (nonPermeableOnly && typeof nonPermeableOnly === "object") nonPermeableOnly = nonPermeableOnly.ptr;
  return _emscripten_bind_btKinematicCharacterController_cameraSphereSweep_9(self, world, fromX, fromY, fromZ, toX, toY, toZ, radius, nonPermeableOnly);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["castShadowProbeGrid"] = btKinematicCharacterController.prototype.castShadowProbeGrid = function(world, centerX, originY, centerZ, radius, maxDist, n, outBuffer) {
  var self = this.ptr;
  if (world && typeof world === "object") world = world.ptr;
  if (centerX && typeof centerX === "object") centerX = centerX.ptr;
  if (originY && typeof originY === "object") originY = originY.ptr;
  if (centerZ && typeof centerZ === "object") centerZ = centerZ.ptr;
  if (radius && typeof radius === "object") radius = radius.ptr;
  if (maxDist && typeof maxDist === "object") maxDist = maxDist.ptr;
  if (n && typeof n === "object") n = n.ptr;
  if (outBuffer && typeof outBuffer === "object") outBuffer = outBuffer.ptr;
  return _emscripten_bind_btKinematicCharacterController_castShadowProbeGrid_8(self, world, centerX, originY, centerZ, radius, maxDist, n, outBuffer);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getCameraRayHitNormalX"] = btKinematicCharacterController.prototype.getCameraRayHitNormalX = function() {
  var self = this.ptr;
  return _emscripten_bind_btKinematicCharacterController_getCameraRayHitNormalX_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getCameraRayHitNormalY"] = btKinematicCharacterController.prototype.getCameraRayHitNormalY = function() {
  var self = this.ptr;
  return _emscripten_bind_btKinematicCharacterController_getCameraRayHitNormalY_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getCameraRayHitNormalZ"] = btKinematicCharacterController.prototype.getCameraRayHitNormalZ = function() {
  var self = this.ptr;
  return _emscripten_bind_btKinematicCharacterController_getCameraRayHitNormalZ_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["getCameraRayHitNonPermeable"] = btKinematicCharacterController.prototype.getCameraRayHitNonPermeable = function() {
  var self = this.ptr;
  return !!(_emscripten_bind_btKinematicCharacterController_getCameraRayHitNonPermeable_0(self));
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["packState"] = btKinematicCharacterController.prototype.packState = function(outBuffer) {
  var self = this.ptr;
  if (outBuffer && typeof outBuffer === "object") outBuffer = outBuffer.ptr;
  return _emscripten_bind_btKinematicCharacterController_packState_1(self, outBuffer);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["resetForNewRun"] = btKinematicCharacterController.prototype.resetForNewRun = function() {
  var self = this.ptr;
  _emscripten_bind_btKinematicCharacterController_resetForNewRun_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["resetCollisionCache"] = btKinematicCharacterController.prototype.resetCollisionCache = function(world, filterGroup, filterMask) {
  var self = this.ptr;
  if (world && typeof world === "object") world = world.ptr;
  if (filterGroup && typeof filterGroup === "object") filterGroup = filterGroup.ptr;
  if (filterMask && typeof filterMask === "object") filterMask = filterMask.ptr;
  _emscripten_bind_btKinematicCharacterController_resetCollisionCache_3(self, world, filterGroup, filterMask);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btKinematicCharacterController.prototype["__destroy__"] = btKinematicCharacterController.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btKinematicCharacterController___destroy___0(self);
};

// Interface: btPairCachingGhostObject
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btPairCachingGhostObject() {
  this.ptr = _emscripten_bind_btPairCachingGhostObject_btPairCachingGhostObject_0();
  getCache(btPairCachingGhostObject)[this.ptr] = this;
}

btPairCachingGhostObject.prototype = Object.create(btGhostObject.prototype);

btPairCachingGhostObject.prototype.constructor = btPairCachingGhostObject;

btPairCachingGhostObject.prototype.__class__ = btPairCachingGhostObject;

btPairCachingGhostObject.__cache__ = {};

Module["btPairCachingGhostObject"] = btPairCachingGhostObject;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btPairCachingGhostObject.prototype["getCollisionShape"] = btPairCachingGhostObject.prototype.getCollisionShape = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btPairCachingGhostObject_getCollisionShape_0(self), btCollisionShape);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btPairCachingGhostObject.prototype["setActivationState"] = btPairCachingGhostObject.prototype.setActivationState = function(newState) {
  var self = this.ptr;
  if (newState && typeof newState === "object") newState = newState.ptr;
  _emscripten_bind_btPairCachingGhostObject_setActivationState_1(self, newState);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btPairCachingGhostObject.prototype["getWorldTransform"] = btPairCachingGhostObject.prototype.getWorldTransform = function() {
  var self = this.ptr;
  return wrapPointer(_emscripten_bind_btPairCachingGhostObject_getWorldTransform_0(self), btTransform);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btPairCachingGhostObject.prototype["setCollisionFlags"] = btPairCachingGhostObject.prototype.setCollisionFlags = function(flags) {
  var self = this.ptr;
  if (flags && typeof flags === "object") flags = flags.ptr;
  _emscripten_bind_btPairCachingGhostObject_setCollisionFlags_1(self, flags);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btPairCachingGhostObject.prototype["setWorldTransform"] = btPairCachingGhostObject.prototype.setWorldTransform = function(worldTrans) {
  var self = this.ptr;
  if (worldTrans && typeof worldTrans === "object") worldTrans = worldTrans.ptr;
  _emscripten_bind_btPairCachingGhostObject_setWorldTransform_1(self, worldTrans);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btPairCachingGhostObject.prototype["setCollisionShape"] = btPairCachingGhostObject.prototype.setCollisionShape = function(collisionShape) {
  var self = this.ptr;
  if (collisionShape && typeof collisionShape === "object") collisionShape = collisionShape.ptr;
  _emscripten_bind_btPairCachingGhostObject_setCollisionShape_1(self, collisionShape);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btPairCachingGhostObject.prototype["getUserIndex"] = btPairCachingGhostObject.prototype.getUserIndex = function() {
  var self = this.ptr;
  return _emscripten_bind_btPairCachingGhostObject_getUserIndex_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btPairCachingGhostObject.prototype["setUserIndex"] = btPairCachingGhostObject.prototype.setUserIndex = function(index) {
  var self = this.ptr;
  if (index && typeof index === "object") index = index.ptr;
  _emscripten_bind_btPairCachingGhostObject_setUserIndex_1(self, index);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btPairCachingGhostObject.prototype["setUserIndex2"] = btPairCachingGhostObject.prototype.setUserIndex2 = function(index) {
  var self = this.ptr;
  if (index && typeof index === "object") index = index.ptr;
  _emscripten_bind_btPairCachingGhostObject_setUserIndex2_1(self, index);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btPairCachingGhostObject.prototype["getNumOverlappingObjects"] = btPairCachingGhostObject.prototype.getNumOverlappingObjects = function() {
  var self = this.ptr;
  return _emscripten_bind_btPairCachingGhostObject_getNumOverlappingObjects_0(self);
};

/** @suppress {undefinedVars, duplicate} @this{Object} */ btPairCachingGhostObject.prototype["__destroy__"] = btPairCachingGhostObject.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btPairCachingGhostObject___destroy___0(self);
};

// Interface: btGhostPairCallback
/** @suppress {undefinedVars, duplicate} @this{Object} */ function btGhostPairCallback() {
  this.ptr = _emscripten_bind_btGhostPairCallback_btGhostPairCallback_0();
  getCache(btGhostPairCallback)[this.ptr] = this;
}

btGhostPairCallback.prototype = Object.create(WrapperObject.prototype);

btGhostPairCallback.prototype.constructor = btGhostPairCallback;

btGhostPairCallback.prototype.__class__ = btGhostPairCallback;

btGhostPairCallback.__cache__ = {};

Module["btGhostPairCallback"] = btGhostPairCallback;

/** @suppress {undefinedVars, duplicate} @this{Object} */ btGhostPairCallback.prototype["__destroy__"] = btGhostPairCallback.prototype.__destroy__ = function() {
  var self = this.ptr;
  _emscripten_bind_btGhostPairCallback___destroy___0(self);
};

(function() {
  function setupEnums() {
    // $PHY_ScalarType
    Module["PHY_FLOAT"] = _emscripten_enum_PHY_ScalarType_PHY_FLOAT();
    Module["PHY_DOUBLE"] = _emscripten_enum_PHY_ScalarType_PHY_DOUBLE();
    Module["PHY_INTEGER"] = _emscripten_enum_PHY_ScalarType_PHY_INTEGER();
    Module["PHY_SHORT"] = _emscripten_enum_PHY_ScalarType_PHY_SHORT();
    Module["PHY_FIXEDPOINT88"] = _emscripten_enum_PHY_ScalarType_PHY_FIXEDPOINT88();
    Module["PHY_UCHAR"] = _emscripten_enum_PHY_ScalarType_PHY_UCHAR();
  }
  if (runtimeInitialized) setupEnums(); else addOnInit(setupEnums);
})();

// end include: glue.js
// include: /Users/casey/ammo.js/onload.js
Module["CONTACT_ADDED_CALLBACK_SIGNATURE"] = "iiiiiiii";

Module["CONTACT_DESTROYED_CALLBACK_SIGNATURE"] = "ii";

Module["CONTACT_PROCESSED_CALLBACK_SIGNATURE"] = "iiii";

Module["INTERNAL_TICK_CALLBACK_SIGNATURE"] = "vif";

// Reassign global Ammo to the loaded module:
this["Ammo"] = Module;

// end include: /Users/casey/ammo.js/onload.js
// include: postamble_modularize.js
// In MODULARIZE mode we wrap the generated code in a factory function
// and return either the Module itself, or a promise of the module.
// We assign to the `moduleRtn` global here and configure closure to see
// this as and extern so it won't get minified.
moduleRtn = readyPromise;


  return moduleRtn;
}
);
})();
if (typeof exports === 'object' && typeof module === 'object') {
  module.exports = Ammo;
  // This default export looks redundant, but it allows TS to import this
  // commonjs style module.
  module.exports.default = Ammo;
} else if (typeof define === 'function' && define['amd'])
  define([], () => Ammo);
export {Ammo};
