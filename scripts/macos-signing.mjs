// Explicit signing modes. No certificate import, credential reading or fallback
// from a requested Developer ID build to ad-hoc signing.
export function packagingOptions(args) {
  const options = {
    channel: 'local-trial',
    preflight: false,
    identity: null,
    profile: null,
    notarize: false,
  };
  const seen = new Set();
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (seen.has(arg)) throw Error('Duplicate packaging option');
    seen.add(arg);
    if (arg === '--prerelease') options.channel = 'public-prerelease';
    else if (arg === '--preflight') options.preflight = true;
    else if (arg === '--notarize') options.notarize = true;
    else if (arg === '--developer-id') options.identity = args[++i];
    else if (arg === '--notary-profile') options.profile = args[++i];
    else throw Error('Unknown packaging option');
  }
  if (options.identity !== null || options.profile !== null || options.notarize) {
    if (
      options.channel !== 'public-prerelease' ||
      !options.notarize ||
      !/^[A-Fa-f0-9]{40}$/.test(options.identity ?? '') ||
      !/^[A-Za-z0-9._-]{1,80}$/.test(options.profile ?? '')
    )
      throw Error(
        'Developer ID requires --prerelease --developer-id <40-hex fingerprint> --notary-profile <Keychain alias> --notarize',
      );
    options.identity = options.identity.toUpperCase();
  }
  return options;
}

export function signingPreflight(options, run) {
  if (!options.notarize) return { mode: 'ad-hoc', notarization: 'NOT_REQUESTED' };
  const inventory = run('/usr/bin/security', ['find-identity', '-v', '-p', 'codesigning']);
  const identities = [
    ...inventory.matchAll(/\b([A-Fa-f0-9]{40})\s+"(Developer ID Application:[^"]+)"/g),
  ];
  if (!identities.some((m) => m[1].toUpperCase() === options.identity))
    throw Error(
      'Requested valid Developer ID Application identity is unavailable; no build, signing or upload performed. Apple Development cannot replace it.',
    );
  run('/usr/bin/xcrun', ['--find', 'notarytool']);
  run('/usr/bin/xcrun', ['--find', 'stapler']);
  return { mode: 'developer-id', notarization: 'REQUESTED_NOT_SUBMITTED', identityAvailable: true };
}

export function signApp(app, options, run) {
  run(
    '/usr/bin/codesign',
    options.notarize
      ? ['--force', '--sign', options.identity, '--options', 'runtime', '--timestamp', app]
      : ['--force', '--sign', '-', '--timestamp=none', app],
  );
  run('/usr/bin/codesign', ['--verify', '--deep', '--strict', app]);
}

export function notarizeTarget(archive, stapleTarget, options, run, record) {
  let response;
  try {
    response = JSON.parse(
      run('/usr/bin/xcrun', [
        'notarytool',
        'submit',
        archive,
        '--keychain-profile',
        options.profile,
        '--wait',
        '--timeout',
        '8m',
        '--output-format',
        'json',
      ]),
    );
  } catch (error) {
    // Keep only non-credential submission identity/status when timeout output
    // contains them. Do not print raw CLI errors or silently resubmit.
    try {
      response = JSON.parse(String(error.stdout));
    } catch {
      /* No response. */
    }
    record({
      id: typeof response?.id === 'string' ? response.id : null,
      status: 'SUBMISSION_INCOMPLETE',
    });
    throw Error(
      'Notarization submission incomplete; retain the archive and submission record for inspection. No automatic retry or ad-hoc fallback.',
    );
  }
  const result = { id: response.id, status: response.status };
  if (!/^[a-f0-9-]{36}$/i.test(result.id ?? '') || result.status !== 'Accepted') {
    record({
      id: typeof result.id === 'string' ? result.id : null,
      status: typeof result.status === 'string' ? result.status : 'INVALID_RESPONSE',
    });
    throw Error(
      'Apple notarization did not return Accepted; package is not ready. Preserve the submission record.',
    );
  }
  record(result);
  run('/usr/bin/xcrun', ['stapler', 'staple', stapleTarget]);
  run('/usr/bin/xcrun', ['stapler', 'validate', stapleTarget]);
  return { ...result, ticket: 'STAPLED_AND_VALIDATED' };
}
