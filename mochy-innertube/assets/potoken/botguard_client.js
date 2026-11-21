/*!
 * BotGuard client for generating poTokens.
 * Updated to fix JavaScript 'this' binding issues and match BgUtils
 * v3.2.0 patterns.
 *
 * ------------------------------------------------------------------
 * mochy desktop port note (Phase 06):
 * This file is an ADAPTATION of the upstream Android asset
 * `app/src/main/assets/po_token.html` (the <script> body only,
 * verbatim) for a non-WebView JS engine (QuickJS-NG via rquickjs).
 * The BotGuard functions themselves — loadBotGuard, snapshot,
 * runBotGuard, createPoTokenMinter, obtainPoToken — are unchanged.
 * What changed, and why:
 *   - the HTML wrapper, the XMLHttpRequest-driven
 *     `downloadAndRunBotguard` and the `PoTokenWebView`
 *     JavascriptInterface callbacks are GONE: the Rust side
 *     (mochy-innertube/src/stream/potoken.rs) makes the Create /
 *     GenerateIT HTTP calls itself and feeds the results in;
 *   - the glue block at the end (clearly marked) replaces the
 *     JavascriptInterface bridge with completion globals
 *     (`__bgDone`) the engine polls while it drains the job queue
 *     and the cooperative timers.
 * License: GPL-3.0-only, same as the upstream repository this was
 * copied from (Mochy Music, pin bde01cb4); the upstream script is
 * itself a re-expression of LuanRT's BgUtils client
 * (https://github.com/LuanRT/BgUtils) patterns.
 */

// Global state for BotGuard
var bgVmFunctions = null;
var bgVm = null;
var bgProgram = null;
var poTokenMinter = null;  // Minter callback - created ONCE during init, reused for all tokens

function loadBotGuard(challengeData) {
  bgVm = window[challengeData.globalName];
  bgProgram = challengeData.program;
  bgVmFunctions = null;

  if (!bgVm)
    throw new Error('[BotGuardClient]: VM not found in the global object');

  if (!bgVm.a)
    throw new Error('[BotGuardClient]: Could not load program');

  // Use explicit variable capture instead of 'this' to avoid binding issues
  var vmFunctionsCallback = function (
    asyncSnapshotFunction,
    shutdownFunction,
    passEventFunction,
    checkCameraFunction
  ) {
    bgVmFunctions = {
      asyncSnapshotFunction: asyncSnapshotFunction,
      shutdownFunction: shutdownFunction,
      passEventFunction: passEventFunction,
      checkCameraFunction: checkCameraFunction
    };
  };

  // Execute the BotGuard program
  try {
    bgVm.a(bgProgram, vmFunctionsCallback, true, undefined, function () {/** no-op */ }, [ [], [] ]);
  } catch (e) {
    throw new Error('[BotGuardClient]: Failed to execute program: ' + e.message);
  }

  // Wait for vmFunctions to be populated (async callback)
  return new Promise(function (resolve, reject) {
    var attempts = 0;
    var maxAttempts = 10000; // 10 seconds at 1ms intervals
    var checkInterval = setInterval(function () {
      if (bgVmFunctions && bgVmFunctions.asyncSnapshotFunction) {
        clearInterval(checkInterval);
        resolve({
          vmFunctions: bgVmFunctions,
          vm: bgVm,
          program: bgProgram
        });
      } else if (attempts >= maxAttempts) {
        clearInterval(checkInterval);
        reject(new Error('[BotGuardClient]: Timeout waiting for asyncSnapshotFunction'));
      }
      attempts++;
    }, 1);
  });
}

/**
 * Takes a snapshot asynchronously using the loaded BotGuard.
 */
function snapshot(botguard, args) {
  return new Promise(function (resolve, reject) {
    if (!botguard.vmFunctions || !botguard.vmFunctions.asyncSnapshotFunction) {
      return reject(new Error('[BotGuardClient]: Async snapshot function not found'));
    }

    try {
      botguard.vmFunctions.asyncSnapshotFunction(
        function (response) { resolve(response); },
        [
          args.contentBinding,
          args.signedTimestamp,
          args.webPoSignalOutput,
          args.skipPrivacyBuffer
        ]
      );
    } catch (e) {
      reject(new Error('[BotGuardClient]: Snapshot failed: ' + e.message));
    }
  });
}

function runBotGuard(challengeData) {
  var interpreterJavascript = challengeData.interpreterJavascript.privateDoNotAccessOrElseSafeScriptWrappedValue;

  if (interpreterJavascript) {
    new Function(interpreterJavascript)();
  } else {
    throw new Error('[BotGuardClient]: Could not load VM - no interpreter JavaScript');
  }

  var webPoSignalOutput = [];

  return loadBotGuard({
    globalName: challengeData.globalName,
    program: challengeData.program
  }).then(function (botguard) {
    return snapshot(botguard, { webPoSignalOutput: webPoSignalOutput });
  }).then(function (botguardResponse) {
    return { webPoSignalOutput: webPoSignalOutput, botguardResponse: botguardResponse };
  });
}

/**
 * Creates the poToken minter callback. MUST be called exactly ONCE during initialization,
 * right after runBotGuard completes. The minter is stored globally and reused for all tokens.
 * NOTE: getMinter() may return a Promise, so this function is async.
 * @param webPoSignalOutput - Array from BotGuard containing the minter factory function
 * @param integrityToken - The integrity token (as bytes/Uint8Array)
 * @returns Promise that resolves when minter is ready
 */
async function createPoTokenMinter(webPoSignalOutput, integrityToken) {
  // Get the minter factory function from webPoSignalOutput
  var getMinter = webPoSignalOutput[0];

  if (!getMinter) {
    throw new Error('PMD:Undefined - webPoSignalOutput[0] is not defined');
  }

  if (typeof getMinter !== 'function') {
    throw new Error('PMD:NotFunction - webPoSignalOutput[0] is not a function, got: ' + typeof getMinter);
  }

  // Create the mint callback by passing the integrity token - THIS MUST ONLY HAPPEN ONCE!
  // NOTE: getMinter() may return a Promise, so we await it
  var mintCallback;
  try {
    var minterResult = getMinter(integrityToken);
    // Handle both sync and async getMinter
    if (minterResult && typeof minterResult.then === 'function') {
      mintCallback = await minterResult;
    } else {
      mintCallback = minterResult;
    }
  } catch (e) {
    throw new Error('GMC:Failed - getMinter() threw: ' + e.message);
  }

  if (!mintCallback) {
    throw new Error('APF:Undefined - mintCallback is undefined');
  }

  if (typeof mintCallback !== 'function') {
    throw new Error('APF:NotFunction - mintCallback is not a function');
  }

  // Store the minter globally for reuse
  poTokenMinter = mintCallback;
}

/**
 * Mints a poToken using the pre-created minter callback.
 * The minter MUST have been created during initialization via createPoTokenMinter().
 * NOTE: mintCallback() may return a Promise, so this function is async.
 * @param identifier - The identifier to bind the token to (as Uint8Array)
 * @returns Promise<Uint8Array> containing the poToken
 */
async function obtainPoToken(identifier) {
  if (!poTokenMinter) {
    throw new Error('MNT:NotInit - poTokenMinter was not initialized. Call createPoTokenMinter first.');
  }

  // Mint the token with the identifier
  // NOTE: mintCallback() may return a Promise, so we await it
  var result;
  try {
    var mintResult = poTokenMinter(identifier);
    // Handle both sync and async mintCallback
    if (mintResult && typeof mintResult.then === 'function') {
      result = await mintResult;
    } else {
      result = mintResult;
    }
  } catch (e) {
    throw new Error('MNT:Failed - mintCallback() threw: ' + e.message);
  }

  if (!result) {
    throw new Error('YNJ:Undefined - mint result is undefined');
  }

  if (!(result instanceof Uint8Array)) {
    throw new Error('ODM:Invalid - result is not Uint8Array, got: ' + Object.prototype.toString.call(result));
  }

  return result;
}

/* ------------------------------------------------------------------ */
/* mochy port glue — replaces the PoTokenWebView JavascriptInterface  */
/* bridge. Each entry starts an async BotGuard step and settles the   */
/* global __bgDone with {ok:true,value:...} or {ok:false,error:...};  */
/* the Rust engine polls it while draining the job queue and the      */
/* cooperative timers.                                                */

globalThis.__bgDone = null;
globalThis.__bgSignalOutput = null;

function __bgSettle(promise) {
  globalThis.__bgDone = null;
  promise.then(
    function (value) { globalThis.__bgDone = { ok: true, value: value }; },
    function (error) {
      globalThis.__bgDone = {
        ok: false,
        error: String((error && error.message) || error)
      };
    }
  );
  return true;
}

globalThis.__bgRun = function (challengeDataJson) {
  globalThis.__bgDone = null;
  try {
    var challengeData = JSON.parse(challengeDataJson);
    __bgSettle(runBotGuard(challengeData).then(function (result) {
      globalThis.__bgSignalOutput = result.webPoSignalOutput;
      return result.botguardResponse;
    }));
  } catch (e) {
    globalThis.__bgDone = { ok: false, error: String((e && e.message) || e) };
  }
  return true;
};

globalThis.__bgCreateMinter = function (integrityTokenBytesJson) {
  globalThis.__bgDone = null;
  try {
    var u8 = new Uint8Array(JSON.parse(integrityTokenBytesJson));
    __bgSettle(createPoTokenMinter(globalThis.__bgSignalOutput, u8));
  } catch (e) {
    globalThis.__bgDone = { ok: false, error: String((e && e.message) || e) };
  }
  return true;
};

globalThis.__bgMint = function (identifierBytesJson) {
  globalThis.__bgDone = null;
  try {
    var u8 = new Uint8Array(JSON.parse(identifierBytesJson));
    __bgSettle(obtainPoToken(u8).then(function (token) {
      return JSON.stringify(Array.from(token));
    }));
  } catch (e) {
    globalThis.__bgDone = { ok: false, error: String((e && e.message) || e) };
  }
  return true;
};
