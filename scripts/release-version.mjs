// Keep the app, package managers and distributable identity in agreement.
export function releaseVersion({ pkg, lock, tauri, cargoToml, cargoLock }) {
  const cargo = cargoToml.split(/^\[package\][ \t]*$/m)[1]?.split(/^\[/m)[0];
  const cargoVersion = cargo?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  const locked = cargoLock
    .split(/^\[\[package\]\]\s*$/m)
    .find((section) => /^name\s*=\s*"moment-grove"\s*$/m.test(section));
  const versions = [
    pkg.version,
    lock.version,
    lock.packages?.['']?.version,
    tauri.version,
    cargoVersion,
    locked?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1],
  ];
  if (
    !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(pkg.version ?? '') ||
    versions.some((v) => v !== pkg.version)
  ) {
    throw Error('Release version mismatch across npm, Cargo and Tauri manifests');
  }
  if (pkg.name !== 'moment-grove' || tauri.identifier !== 'app.momentgrove.desktop')
    throw Error('Release product identity mismatch');
  return pkg.version;
}

export function releaseArchiveStem(version, buildId, channel) {
  return `Moment-Grove-${version}${channel === 'local-trial' ? `-${buildId}` : ''}-macOS-arm64`;
}
